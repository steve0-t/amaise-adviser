use ameise_adviser::{Store, project_root, validation};
use serde_json::{Value, json};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture {
    store: Store,
    dir: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!(
            "ameise-rust-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        Self {
            store: Store::open(&dir, &project_root()).unwrap(),
            dir,
        }
    }
    fn advice(&mut self) -> Value {
        let state = self.store.snapshot(Some("stream"), false).unwrap();
        json!({"api_version":"2","advice_id":"a1","stream_id":"stream","based_on_revision":state["revision"],"policy_version":"qa200-1.95-v1","source":"agent","summary":"Inspect observed state","observations":[{"text":"Game selected","evidence_event_ids":["e1"]}],"recommendations":[{"command_id":"show_specification","arguments":{},"reason":"Read information","evidence_event_ids":["e1"]}],"missing_information":[]})
    }
    fn reject_advice(&mut self, a: &Value, code: &str) {
        assert_eq!(self.store.submit(a).unwrap_err().code, code);
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}
fn event(seq: i64, kind: &str) -> Value {
    let p = match kind {
        "game.selected" => json!({"name":"QA 200","simulation_date_raw":null}),
        "command.sent" => json!({"request_id":"request","command":"show me specification"}),
        "command.result" => {
            json!({"request_id":"request","command":"show me specification","feedback":"Observed result","kernel_output_raw":"","simulation_date_raw":null})
        }
        "callback.requested" => {
            json!({"request_id":"request","command":"show me specification","question":"Choose","choices":[],"alternatives_raw":"unparsed","previous_answer_ids":[12,3]})
        }
        _ => {
            json!({"request_id":"request","command":"show me specification","message":"Connection lost"})
        }
    };
    json!({"api_version":"2","event_id":format!("e{seq}"),"stream_id":"stream","sequence":seq,"occurred_at":"2026-10-28T16:00:00Z","source":"import","kind":kind,"game":{"game_id":17,"tournament_id":4,"model_instance_id":9},"turn_id":42,"payload":p})
}
#[test]
fn empty_and_demo() {
    let mut f = Fixture::new();
    let s = f.store.snapshot(None, true).unwrap();
    assert_eq!(s["revision"], 0);
    assert!(s["game"].is_null());
    assert_eq!(s["events"], json!([]));
    assert_eq!(s["quality"]["uncertain"], false);
    let ex = f.store.examples();
    for e in ex["events"].as_array().unwrap() {
        assert_eq!(f.store.ingest(e).unwrap().0, 201);
    }
    let (_, a) = f.store.submit(&ex["advice"]).unwrap();
    assert_eq!(
        a["rendered_recommendations"][0]["command"],
        "show me review report"
    );
}
#[test]
fn malformed_and_strict_json() {
    let mut f = Fixture::new();
    for (pointer, value) in [
        ("/credentials", json!({"password":"forbidden"})),
        ("/sequence", json!(true)),
        ("/sequence", json!(9223372036854775808u64)),
        ("/game/game_id", json!(0)),
        ("/occurred_at", json!("2026-02-30T10:00:00Z")),
        ("/occurred_at", json!("2026-10-28")),
        ("/payload/extra", json!("invalid")),
    ] {
        let mut e = event(1, "game.selected");
        if pointer == "/credentials" {
            e["credentials"] = value;
        } else if pointer == "/payload/extra" {
            e["payload"]["extra"] = value;
        } else {
            *e.pointer_mut(pointer).unwrap() = value;
        }
        assert_eq!(f.store.ingest(&e).unwrap_err().code, "validation_error");
    }
    for e in [json!({}), json!([]), Value::Null] {
        assert!(f.store.ingest(&e).is_err());
    }
    for text in [
        b"\xff".as_slice(),
        b"{\"x\":1,\"x\":2}",
        b"{\"x\":NaN}",
        b"{\"nested\":{\"x\":1,\"x\":2}}",
        b"{\"x\":Infinity}",
    ] {
        assert_eq!(validation::parse(text).unwrap_err().code, "invalid_json");
    }
    assert_eq!(f.store.snapshot(None, false).unwrap()["revision"], 0);
}
#[test]
fn dedup_conflicts_and_scoping() {
    let mut f = Fixture::new();
    let e = event(1, "game.selected");
    f.store.ingest(&e).unwrap();
    let (status, ack) = f.store.ingest(&e).unwrap();
    assert_eq!(status, 200);
    assert_eq!(ack["revision"], 1);
    assert_eq!(ack["duplicate"], true);
    let mut changed = e.clone();
    changed["turn_id"] = 1.into();
    assert_eq!(f.store.ingest(&changed).unwrap_err().code, "event_conflict");
    changed = e.clone();
    changed["event_id"] = "other".into();
    assert_eq!(
        f.store.ingest(&changed).unwrap_err().code,
        "sequence_conflict"
    );
    changed = event(2, "game.selected");
    changed["game"]["game_id"] = 2.into();
    assert_eq!(f.store.ingest(&changed).unwrap_err().code, "game_conflict");
    changed = e.clone();
    changed["stream_id"] = "other".into();
    assert_eq!(f.store.ingest(&changed).unwrap_err().code, "event_conflict");
    changed["event_id"] = "other".into();
    f.store.ingest(&changed).unwrap();
    assert_eq!(
        f.store.snapshot(None, false).unwrap()["selected_stream_id"],
        "other"
    );
    assert_eq!(
        f.store.snapshot(Some("stream"), false).unwrap()["events"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        f.store.snapshot(Some("unknown"), false).unwrap_err().status,
        404
    );
}
#[test]
fn policies_and_arguments() {
    let mut f = Fixture::new();
    f.store.ingest(&event(1, "game.selected")).unwrap();
    for c in f.store.policy["commands"].as_array().unwrap().clone() {
        if c["recommendation"] != "allowed" {
            let mut a = f.advice();
            a["recommendations"][0]["command_id"] = c["id"].clone();
            f.reject_advice(&a, "command_not_allowed");
        }
    }
    for (id, args) in [
        ("show_activity", json!({"developer":"Fabricated"})),
        ("show_activity", json!({})),
        (
            "show_activity",
            json!({"developer":"Bernd","command":"fire Axel"}),
        ),
        ("proceed_one_day", json!({"days":true})),
        ("proceed_one_day", json!({"days":2})),
        ("proceed_one_day", json!({"days":"1"})),
        (
            "review_two_developers",
            json!({"reviewer_1":"Bernd","reviewer_2":"Bernd","document":"specification"}),
        ),
        (
            "review_two_developers",
            json!({"reviewer_1":"Bernd","reviewer_2":"Stefanie","document":"invented"}),
        ),
        (
            "correct_tested_code",
            json!({"developer":"Axel","report_name":"invented-report"}),
        ),
    ] {
        let mut a = f.advice();
        a["recommendations"][0]["command_id"] = id.into();
        a["recommendations"][0]["arguments"] = args;
        f.reject_advice(&a, "invalid_arguments");
    }
    let mut a = f.advice();
    a["recommendations"][0]["command"] = "finish project".into();
    f.reject_advice(&a, "validation_error");
    let mut a = f.advice();
    a["recommendations"][0]["command_id"] = "unknown".into();
    f.reject_advice(&a, "command_not_allowed");
}
#[test]
fn rendered_templates_and_report_evidence() {
    let mut f = Fixture::new();
    f.store.ingest(&event(1, "game.selected")).unwrap();
    let mut a = f.advice();
    a["recommendations"][0]["command_id"] = "review_three_developers".into();
    a["recommendations"][0]["arguments"] = json!({"reviewer_1":"Bernd","reviewer_2":"Stefanie","reviewer_3":"Axel","document":"specification"});
    assert_eq!(
        f.store.submit(&a).unwrap().1["rendered_recommendations"][0]["command"],
        "instruct Bernd and Stefanie and Axel review specification"
    );
    let mut e = event(2, "command.result");
    e["payload"]["kernel_output_raw"] = "Generated report module-test-17".into();
    f.store.ingest(&e).unwrap();
    let mut a = f.advice();
    a["advice_id"] = "a2".into();
    a["recommendations"][0] = json!({"command_id":"correct_tested_code","arguments":{"developer":"Axel","report_name":"module-test-17"},"reason":"Read result","evidence_event_ids":["e2"]});
    assert_eq!(
        f.store.submit(&a).unwrap().1["rendered_recommendations"][0]["command"],
        "Axel correct tested code using module-test-17"
    );
    a["recommendations"][0]["evidence_event_ids"] = json!(["e1"]);
    f.reject_advice(&a, "invalid_arguments");
    for text in ["bad\ncommand", "bad\tname", "bad\u{7f}name", " "] {
        a["recommendations"][0]["arguments"]["report_name"] = text.into();
        f.reject_advice(&a, "invalid_arguments");
    }
}
#[test]
fn evidence_and_advice_identity() {
    let mut f = Fixture::new();
    f.store.ingest(&event(1, "game.selected")).unwrap();
    let mut other = event(1, "game.selected");
    other["event_id"] = "foreign".into();
    other["stream_id"] = "other".into();
    f.store.ingest(&other).unwrap();
    for field in ["observations", "recommendations"] {
        for id in ["foreign", "missing"] {
            let mut a = f.advice();
            a[field][0]["evidence_event_ids"] = json!([id]);
            f.reject_advice(&a, "invalid_evidence");
        }
    }
    let mut a = f.advice();
    assert_eq!(f.store.submit(&a).unwrap().0, 201);
    assert_eq!(f.store.submit(&a).unwrap().0, 200);
    a["summary"] = "Changed".into();
    f.reject_advice(&a, "advice_conflict");
    f.store.ingest(&event(1, "game.selected")).unwrap();
    assert_eq!(
        f.store.snapshot(Some("stream"), false).unwrap()["advice"][0]["stale"],
        false
    );
    f.store.ingest(&event(2, "game.selected")).unwrap();
    assert_eq!(
        f.store.snapshot(Some("stream"), false).unwrap()["advice"][0]["stale"],
        true
    );
    f.reject_advice(&a, "stale_advice");
}
#[test]
fn pending_callbacks_failures_and_mismatch() {
    for kind in ["command.sent", "callback.requested", "transport.uncertain"] {
        let mut f = Fixture::new();
        f.store.ingest(&event(1, "game.selected")).unwrap();
        f.store.ingest(&event(2, kind)).unwrap();
        let a = f.advice();
        f.reject_advice(&a, "context_uncertain");
        let mut notes = a.clone();
        notes["recommendations"] = json!([]);
        f.store.submit(&notes).unwrap();
        f.store.ingest(&event(3, "command.result")).unwrap();
        assert_eq!(
            f.store.snapshot(None, false).unwrap()["quality"]["uncertain"],
            false
        );
        let mut a = f.advice();
        a["advice_id"] = "a2".into();
        f.store.submit(&a).unwrap();
        let mut e = event(4, "command.result");
        e["payload"]["command"] = "different".into();
        f.store.ingest(&e).unwrap();
        assert_eq!(
            f.store.snapshot(None, false).unwrap()["quality"]["uncertain"],
            true
        );
    }
}
#[test]
fn gaps_late_events_and_bounded_details() {
    let mut f = Fixture::new();
    f.store.ingest(&event(1, "game.selected")).unwrap();
    f.store.ingest(&event(3, "command.result")).unwrap();
    assert_eq!(
        f.store.snapshot(None, false).unwrap()["quality"]["missing_sequences"],
        json!([2])
    );
    let mut a = f.advice();
    a["recommendations"] = json!([]);
    f.store.submit(&a).unwrap();
    let mut late = event(2, "command.sent");
    late["turn_id"] = 0.into();
    f.store.ingest(&late).unwrap();
    let s = f.store.snapshot(None, true).unwrap();
    assert_eq!(s["revision"], 3);
    assert_eq!(s["turn_id"], 42);
    assert_eq!(s["quality"]["uncertain"], false);
    assert_eq!(s["advice"][0]["stale"], true);
    f.store.ingest(&event(1000000, "game.selected")).unwrap();
    let s = f.store.snapshot(None, false).unwrap();
    assert_eq!(
        s["quality"]["missing_sequences"].as_array().unwrap().len(),
        1000
    );
    assert_eq!(s["quality"]["truncated"], true);
}
#[test]
fn unknown_commands_are_observable_and_state_survives_restart() {
    let mut f = Fixture::new();
    f.store.ingest(&event(1, "game.selected")).unwrap();
    let a = f.advice();
    f.store.submit(&a).unwrap();
    for (seq, cmd) in [(2, "finish project"), (3, "unknown upstream command")] {
        let mut e = event(seq, "command.sent");
        e["payload"]["command"] = cmd.into();
        f.store.ingest(&e).unwrap();
    }
    let mut reopened = Store::open(&f.dir, &project_root()).unwrap();
    assert_eq!(
        reopened.snapshot(None, false).unwrap()["events"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    assert_eq!(
        reopened.snapshot(None, false).unwrap()["advice"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(reopened.ingest(&event(1, "game.selected")).unwrap().0, 200);
}
#[test]
fn existing_python_database_uses_same_tables_and_semantic_json() {
    let mut f = Fixture::new();
    // Python canonical JSON escapes control bytes differently but has the same values.
    let e = event(1, "game.selected");
    f.store
        .db
        .execute(
            "INSERT INTO streams VALUES ('stream',?,1,'2026-10-28T16:00:00.000000Z')",
            [serde_json::to_string_pretty(&e["game"]).unwrap()],
        )
        .unwrap();
    f.store
        .db
        .execute(
            "INSERT INTO events VALUES ('e1','stream',1,?)",
            [serde_json::to_string_pretty(&e).unwrap()],
        )
        .unwrap();
    assert_eq!(f.store.ingest(&e).unwrap().0, 200);
    let a = f.advice();
    f.store.submit(&a).unwrap();
    assert_eq!(f.store.snapshot(None, false).unwrap()["revision"], 1);
}

#[test]
#[ignore = "Run by client/test.sh after it generates Java protocol fixtures"]
fn generated_java_packets_are_valid() {
    let mut f = Fixture::new();
    let text =
        std::fs::read_to_string(project_root().join("adviser/client/.build/fixture-events.jsonl"))
            .unwrap();
    for line in text.lines() {
        let packet = validation::parse(line.as_bytes()).unwrap();
        f.store.ingest(&packet).unwrap();
    }
    let s = f.store.snapshot(None, false).unwrap();
    assert_eq!(s["streams"].as_array().unwrap().len(), 2);
    for stream in s["streams"].as_array().unwrap() {
        let state = f
            .store
            .snapshot(stream["stream_id"].as_str(), false)
            .unwrap();
        assert_eq!(state["quality"]["uncertain"], false);
    }
}
