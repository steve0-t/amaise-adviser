use ameise_adviser::{
    Store, http, project_root,
    runner::{Runner, invoke, reviewed, revision},
};
use serde_json::{Value, json};
use std::{
    collections::HashSet,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};
static NEXT: AtomicU64 = AtomicU64::new(0);
#[test]
fn saved_reviews_are_revision_and_policy_scoped() {
    let mut c = json!({"selected_stream_id":"s","revision":3,"policy_version":"v1","advice":[{"source":"agent","based_on_revision":3,"policy_version":"v1"}]});
    assert!(reviewed(&c));
    let key = revision(&c).unwrap();
    c["revision"] = 4.into();
    assert!(!reviewed(&c));
    assert_ne!(revision(&c).unwrap(), key);
    c["revision"] = 3.into();
    c["policy_version"] = "v2".into();
    assert!(!reviewed(&c));
    c["selected_stream_id"] = Value::Null;
    assert!(revision(&c).is_none());
}
#[tokio::test]
async fn model_timeout_and_invalid_output() {
    let err = invoke(
        &[
            "sh".into(),
            "-c".into(),
            "cat >/dev/null; exec sleep 10".into(),
        ],
        &json!({}),
        Duration::from_millis(40),
    )
    .await
    .unwrap_err();
    assert_eq!(err.code, "model_timeout");
    assert!(
        invoke(
            &[
                "sh".into(),
                "-c".into(),
                "cat >/dev/null; printf invalid".into()
            ],
            &json!({}),
            Duration::from_secs(2)
        )
        .await
        .is_err()
    );
}
#[tokio::test]
async fn continuous_review_restart_and_stale_drafts() {
    let root = project_root();
    let dir = std::env::temp_dir().join(format!(
        "ameise-runner-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let mut store = Store::open(&dir, &root).unwrap();
    let examples = store.examples();
    for e in examples["events"].as_array().unwrap() {
        store.ingest(e).unwrap();
    }
    let mut advice = examples["advice"].clone();
    advice["source"] = "agent".into();
    std::fs::write(dir.join("advice.json"), advice.to_string()).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let shared = Arc::new(Mutex::new(store));
    // Use the production router; keep a second SQLite connection to simulate new packets.
    let service_store = Store::open(&dir, &root).unwrap();
    let fail_once = Arc::new(AtomicBool::new(true));
    let task = tokio::spawn(async move {
        let router = http::router(service_store, addr.port()).layer(axum::middleware::from_fn(
            move |request: axum::extract::Request, next: axum::middleware::Next| {
                let fail_once = fail_once.clone();
                async move {
                    if request.method() == axum::http::Method::POST
                        && fail_once.swap(false, Ordering::SeqCst)
                    {
                        use axum::response::IntoResponse;
                        return (
                            axum::http::StatusCode::SERVICE_UNAVAILABLE,
                            axum::Json(json!({"error":{"code":"unavailable"}})),
                        )
                            .into_response();
                    }
                    next.run(request).await
                }
            },
        ));
        axum::serve(listener, router).await.unwrap();
    });
    let mut runner = Runner {
        client: reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(3))
            .build()
            .unwrap(),
        base: format!("http://{addr}"),
        command: vec![
            "sh".into(),
            "-c".into(),
            "cat >/dev/null; cat \"$1\"; printf x >>\"$2\"".into(),
            "adapter".into(),
            dir.join("advice.json").to_string_lossy().into(),
            dir.join("calls").to_string_lossy().into(),
        ],
        instructions: "Return JSON advice".into(),
        schema: serde_json::from_slice(
            &std::fs::read(root.join("docs/adviser-advice.schema.json")).unwrap(),
        )
        .unwrap(),
        rules: String::new(),
        quiet: Duration::from_millis(5),
        model_timeout: Duration::from_secs(3),
        seen: HashSet::new(),
        pending: None,
    };
    assert_eq!(runner.tick().await.unwrap_err().code, "unavailable");
    let pending_id = runner.pending.as_ref().unwrap()["advice_id"].clone();
    runner.tick().await.unwrap();
    runner.tick().await.unwrap();
    assert_eq!(
        shared.lock().unwrap().snapshot(None, false).unwrap()["advice"][0]["advice_id"],
        pending_id
    );
    assert_eq!(std::fs::read_to_string(dir.join("calls")).unwrap(), "x");
    assert_eq!(
        shared.lock().unwrap().snapshot(None, false).unwrap()["advice"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    runner.seen.clear();
    runner.tick().await.unwrap();
    assert_eq!(std::fs::read_to_string(dir.join("calls")).unwrap(), "x");
    let mut next = examples["events"][0].clone();
    let n = examples["events"].as_array().unwrap().len() as i64 + 1;
    next["sequence"] = n.into();
    next["event_id"] = "extra1".into();
    shared.lock().unwrap().ingest(&next).unwrap();
    advice["based_on_revision"] = n.into();
    std::fs::write(dir.join("advice.json"), advice.to_string()).unwrap();
    runner.command[2] = "cat >/dev/null; printf x >>\"$2\"; sleep 0.2; cat \"$1\"".into();
    let shared2 = shared.clone();
    let mut later = next.clone();
    later["sequence"] = (n + 1).into();
    later["event_id"] = "extra2".into();
    let change = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(80)).await;
        shared2.lock().unwrap().ingest(&later).unwrap();
    });
    runner.tick().await.unwrap();
    change.await.unwrap();
    assert_eq!(std::fs::read_to_string(dir.join("calls")).unwrap(), "xx");
    assert_eq!(
        shared.lock().unwrap().snapshot(None, false).unwrap()["advice"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    advice["based_on_revision"] = (n + 1).into();
    std::fs::write(dir.join("advice.json"), advice.to_string()).unwrap();
    runner.tick().await.unwrap();
    runner.tick().await.unwrap();
    assert_eq!(std::fs::read_to_string(dir.join("calls")).unwrap(), "xxx");
    // Simulate a saved successful response whose acknowledgement was lost.
    let saved = shared.lock().unwrap().snapshot(None, false).unwrap()["advice"][0].clone();
    let mut retry = saved.clone();
    for key in ["created_at", "stale", "rendered_recommendations"] {
        retry.as_object_mut().unwrap().remove(key);
    }
    runner.pending = Some(retry);
    runner.tick().await.unwrap();
    assert!(runner.pending.is_none());
    assert_eq!(std::fs::read_to_string(dir.join("calls")).unwrap(), "xxx");
    task.abort();
    let _ = task.await;
    drop(shared);
    let _ = std::fs::remove_dir_all(dir);
}
