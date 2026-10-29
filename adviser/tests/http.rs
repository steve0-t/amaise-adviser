use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader},
    path::PathBuf,
    process::{Child, Command, Stdio},
    time::Duration,
};
struct Server {
    child: Child,
    dir: PathBuf,
    base: String,
}
impl Server {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("ameise-http-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut child = Command::new(env!("CARGO_BIN_EXE_ameise-adviser"))
            .args(["--port", "0", "--data-dir"])
            .arg(&dir)
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut line = String::new();
        BufReader::new(child.stdout.take().unwrap())
            .read_line(&mut line)
            .unwrap();
        let base = line
            .split_whitespace()
            .nth(2)
            .expect("receiver must bind loopback and announce URL")
            .to_owned();
        Self { child, dir, base }
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}
#[tokio::test]
async fn rust_binary_http_contract() {
    let server = Server::new();
    let c = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap();
    let get = |path: &str| c.get(format!("{}{path}", server.base));
    let post = |path: &str| c.post(format!("{}{path}", server.base));
    let health: Value = get("/api/v2/health")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        health,
        json!({"status":"ok","mode":"adviser","simulator_control":false})
    );
    let state: Value = get("/api/v2/context")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(state["revision"], 0);
    for path in [
        "/execute",
        "/api/v2/execute",
        "/api/v2/commands",
        "/api/v2/login",
        "/api/v2/answer_callback",
    ] {
        assert_eq!(
            post(path).json(&json!({})).send().await.unwrap().status(),
            404
        );
    }
    for path in [
        "/login.txt",
        "/docs/%2e%2e/login.txt",
        "/adviser/server.py",
        "/.runtime/adviser.sqlite3",
    ] {
        assert_eq!(get(path).send().await.unwrap().status(), 404);
    }
    for origin in ["https://example.com", "null", "http://localhost:1"] {
        assert_eq!(
            get("/api/v2/state")
                .header("Origin", origin)
                .send()
                .await
                .unwrap()
                .status(),
            403
        );
    }
    assert_eq!(
        get("/api/v2/state")
            .header("Host", "attacker.example")
            .send()
            .await
            .unwrap()
            .status(),
        400
    );
    assert_eq!(
        get("/api/v2/state")
            .header("Origin", &server.base)
            .send()
            .await
            .unwrap()
            .status(),
        200
    );
    for path in [
        "/api/v2/state?stream_id=",
        "/api/v2/state?stream_id=a&stream_id=b",
        "/api/v2/state?password=x",
    ] {
        assert_eq!(get(path).send().await.unwrap().status(), 400);
    }
    for body in [
        "{\"bad\":",
        "{\"kind\":1,\"kind\":2}",
        "{\"bad\":NaN}",
        "[1,2]",
    ] {
        assert_eq!(
            post("/api/v2/events")
                .header("Content-Type", "application/json")
                .body(body)
                .send()
                .await
                .unwrap()
                .status(),
            400
        );
    }
    assert_eq!(
        post("/api/v2/events")
            .header("Content-Type", "application/json")
            .body(" ".repeat(1024 * 1024 + 1))
            .send()
            .await
            .unwrap()
            .status(),
        413
    );
    assert_eq!(
        post("/api/v2/events")
            .header("Content-Type", "text/plain")
            .body("{}")
            .send()
            .await
            .unwrap()
            .status(),
        400
    );
    let response = get("/").send().await.unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(response.headers()["X-Content-Type-Options"], "nosniff");
    assert!(
        response.headers()["Content-Security-Policy"]
            .to_str()
            .unwrap()
            .contains("frame-ancestors 'none'")
    );
    assert!(response.text().await.unwrap().contains("<html"));
    let examples: Value = get("/api/v2/examples")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    for event in examples["events"].as_array().unwrap() {
        assert_eq!(
            post("/api/v2/events")
                .json(event)
                .send()
                .await
                .unwrap()
                .status(),
            201
        );
    }
    let ack: Value = post("/api/v2/events")
        .json(&examples["events"][0])
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(ack["duplicate"], true);
    assert_eq!(
        post("/api/v2/advice")
            .json(&examples["advice"])
            .send()
            .await
            .unwrap()
            .status(),
        201
    );
    assert_eq!(
        post("/api/v2/advice")
            .json(&examples["advice"])
            .send()
            .await
            .unwrap()
            .status(),
        200
    );
    let mut late = examples["events"][0].clone();
    late["event_id"] = "new-event".into();
    late["sequence"] = 99.into();
    assert_eq!(
        post("/api/v2/events")
            .json(&late)
            .send()
            .await
            .unwrap()
            .status(),
        201
    );
    let response = post("/api/v2/advice")
        .json(&examples["advice"])
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 409);
    assert_eq!(
        response.json::<Value>().await.unwrap()["error"]["code"],
        "stale_advice"
    );
    let state: Value = get("/api/v2/state")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(state["advice"][0]["stale"], true);
    let head = c
        .head(format!("{}/api/v2/health", server.base))
        .send()
        .await
        .unwrap();
    assert_eq!(head.status(), 200);
    assert!(head.bytes().await.unwrap().is_empty());
    let status = Command::new(env!("CARGO_BIN_EXE_ameise-adviser"))
        .args(["--host", "0.0.0.0"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap();
    assert!(!status.success());
}
