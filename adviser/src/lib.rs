pub mod http;
pub mod runner;
pub mod validation;
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};
pub type Result<T> = std::result::Result<T, ApiError>;
#[derive(Debug)]
pub struct ApiError {
    pub status: u16,
    pub code: String,
    pub message: String,
}
impl ApiError {
    pub fn new(status: u16, code: &str, message: impl Into<String>) -> Self {
        Self {
            status,
            code: code.into(),
            message: message.into(),
        }
    }
}
impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}
impl std::error::Error for ApiError {}
impl From<rusqlite::Error> for ApiError {
    fn from(_: rusqlite::Error) -> Self {
        Self::new(
            500,
            "internal_error",
            "The local adviser could not complete the database request.",
        )
    }
}
pub fn project_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .into()
}
pub fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Micros, true)
}
fn read_json(path: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(path).expect("read bundled JSON"))
        .expect("valid bundled JSON")
}
fn decoded(text: String) -> Value {
    serde_json::from_str(&text).expect("valid stored JSON")
}

pub fn quality(events: &[Value]) -> Value {
    let mut missing = Vec::new();
    let mut count = 0i64;
    let mut expected = 1i64;
    let mut requests: BTreeMap<&str, (BTreeSet<&str>, bool)> = BTreeMap::new();
    for e in events {
        let seq = e["sequence"].as_i64().unwrap();
        if seq > expected {
            count += seq - expected;
            missing
                .extend(expected..expected + (seq - expected).min((1000 - missing.len()) as i64));
        }
        expected = seq.saturating_add(1);
        if e["kind"] == "game.selected" {
            continue;
        }
        let p = &e["payload"];
        let r = requests
            .entry(p["request_id"].as_str().unwrap())
            .or_default();
        r.0.insert(p["command"].as_str().unwrap());
        r.1 = e["kind"] != "command.result";
    }
    json!({"missing_sequences":missing,"uncertain": count > 0 || requests.values().any(|(c,p)| *p || c.len()!=1),"truncated":count>1000})
}
pub struct Store {
    pub db: Connection,
    pub root: PathBuf,
    pub policy: Value,
    event_schema: Value,
    advice_schema: Value,
}
impl Store {
    pub fn open(data: &Path, root: &Path) -> Result<Self> {
        std::fs::create_dir_all(data)
            .map_err(|_| ApiError::new(500, "storage_error", "Cannot create data directory"))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(data, std::fs::Permissions::from_mode(0o700)).map_err(
                |_| ApiError::new(500, "storage_error", "Cannot protect data directory"),
            )?;
        }
        let db = Connection::open(data.join("adviser.sqlite3"))?;
        db.busy_timeout(std::time::Duration::from_secs(5))?;
        db.execute_batch("PRAGMA journal_mode=WAL;
            CREATE TABLE IF NOT EXISTS streams (stream_id TEXT PRIMARY KEY, game TEXT NOT NULL, revision INTEGER NOT NULL, last_seen_at TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS events (event_id TEXT PRIMARY KEY, stream_id TEXT NOT NULL, sequence INTEGER NOT NULL, document TEXT NOT NULL, UNIQUE(stream_id, sequence));
            CREATE TABLE IF NOT EXISTS advice (advice_id TEXT PRIMARY KEY, stream_id TEXT NOT NULL, document TEXT NOT NULL, created_at TEXT NOT NULL);")?;
        Ok(Self {
            db,
            root: root.into(),
            policy: read_json(&root.join("docs/adviser-command-policy.json")),
            event_schema: read_json(&root.join("docs/adviser-event.schema.json")),
            advice_schema: read_json(&root.join("docs/adviser-advice.schema.json")),
        })
    }
    pub fn ingest(&mut self, event: &Value) -> Result<(u16, Value)> {
        validation::validate(event, &self.event_schema)?;
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let id = event["event_id"].as_str().unwrap();
        let stream = event["stream_id"].as_str().unwrap();
        let old: Option<String> = tx
            .query_row("SELECT document FROM events WHERE event_id=?", [id], |r| {
                r.get(0)
            })
            .optional()?;
        let existing: Option<(String, i64)> = tx
            .query_row(
                "SELECT game,revision FROM streams WHERE stream_id=?",
                [stream],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        if let Some(old) = old {
            if decoded(old) != *event {
                return Err(ApiError::new(
                    409,
                    "event_conflict",
                    "event_id already exists with different content.",
                ));
            }
            return Ok((
                200,
                json!({"accepted":true,"duplicate":true,"stream_id":stream,"revision":existing.unwrap().1}),
            ));
        }
        if tx
            .query_row(
                "SELECT 1 FROM events WHERE stream_id=? AND sequence=?",
                params![stream, event["sequence"].as_i64()],
                |_| Ok(()),
            )
            .optional()?
            .is_some()
        {
            return Err(ApiError::new(
                409,
                "sequence_conflict",
                "Sequence already exists.",
            ));
        }
        if existing
            .as_ref()
            .is_some_and(|(game, _)| decoded(game.clone()) != event["game"])
        {
            return Err(ApiError::new(
                409,
                "game_conflict",
                "A stream cannot contain different games.",
            ));
        }
        let revision = existing.map_or(1, |(_, r)| r + 1);
        tx.execute(
            "INSERT INTO events VALUES (?,?,?,?)",
            params![id, stream, event["sequence"].as_i64(), event.to_string()],
        )?;
        tx.execute("INSERT INTO streams VALUES (?,?,?,?) ON CONFLICT(stream_id) DO UPDATE SET revision=excluded.revision,last_seen_at=excluded.last_seen_at",params![stream,event["game"].to_string(),revision,now()])?;
        tx.commit()?;
        Ok((
            201,
            json!({"accepted":true,"duplicate":false,"stream_id":stream,"revision":revision}),
        ))
    }
    pub fn snapshot(&mut self, stream: Option<&str>, context: bool) -> Result<Value> {
        let tx = self.db.transaction()?;
        let mut state = snapshot(&tx, &self.policy, stream)?;
        if context {
            state["game"] = state["streams"]
                .as_array()
                .unwrap()
                .iter()
                .find(|s| s["stream_id"] == state["selected_stream_id"])
                .map(|s| s["game"].clone())
                .unwrap_or(Value::Null);
            state["turn_id"] = state["events"]
                .as_array()
                .unwrap()
                .last()
                .map(|e| e["turn_id"].clone())
                .unwrap_or(Value::Null);
            state["policy_version"] = self.policy["policy_version"].clone();
        }
        tx.commit()?;
        Ok(state)
    }
    pub fn submit(&mut self, advice: &Value) -> Result<(u16, Value)> {
        validation::validate(advice, &self.advice_schema)?;
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let state = snapshot(&tx, &self.policy, advice["stream_id"].as_str())?;
        if advice["based_on_revision"] != state["revision"] {
            return Err(ApiError::new(
                409,
                "stale_advice",
                "Advice must use the current ingestion revision.",
            ));
        }
        if advice["policy_version"] != self.policy["policy_version"] {
            return Err(ApiError::new(
                409,
                "policy_conflict",
                "Advice must use the current policy.",
            ));
        }
        let events = state["events"].as_array().unwrap();
        for item in advice["observations"]
            .as_array()
            .unwrap()
            .iter()
            .chain(advice["recommendations"].as_array().unwrap())
        {
            if item["evidence_event_ids"]
                .as_array()
                .unwrap()
                .iter()
                .any(|id| !events.iter().any(|e| &e["event_id"] == id))
            {
                return Err(ApiError::new(
                    400,
                    "invalid_evidence",
                    "Evidence must belong to this stream.",
                ));
            }
        }
        let mut rendered = Vec::new();
        for r in advice["recommendations"].as_array().unwrap() {
            let command = self.policy["commands"]
                .as_array()
                .unwrap()
                .iter()
                .find(|c| c["id"] == r["command_id"] && c["recommendation"] == "allowed")
                .ok_or_else(|| {
                    ApiError::new(
                        400,
                        "command_not_allowed",
                        "Command is not allowed as a recommendation.",
                    )
                })?;
            let args = r["arguments"].as_object().unwrap();
            let parameters = command["parameters"].as_object().unwrap();
            let invalid = |msg: &str| ApiError::new(400, "invalid_arguments", msg);
            if args.keys().ne(parameters.keys()) {
                return Err(invalid("Arguments must match policy parameters exactly."));
            }
            let mut reviewers = BTreeSet::new();
            let mut text = command["template"].as_str().unwrap().to_owned();
            for (name, p) in parameters {
                let arg = &args[name];
                validation::validate(arg, p).map_err(|e| invalid(&e.message))?;
                if let Some(t) = arg.as_str() {
                    if t.trim().is_empty()
                        || t.chars().count() > 256
                        || t.chars().any(|c| c.is_control())
                    {
                        return Err(invalid(
                            "Text arguments must contain 1–256 printable characters.",
                        ));
                    }
                    if name == "report_name"
                        && !events.iter().any(|e| {
                            e["kind"] == "command.result"
                                && r["evidence_event_ids"]
                                    .as_array()
                                    .unwrap()
                                    .contains(&e["event_id"])
                                && ["feedback", "kernel_output_raw"]
                                    .iter()
                                    .any(|f| e["payload"][f].as_str().unwrap().contains(t))
                        })
                    {
                        return Err(invalid(
                            "report_name must occur in cited result feedback or raw output.",
                        ));
                    }
                }
                if name.starts_with("reviewer_") && !reviewers.insert(arg.to_string()) {
                    return Err(invalid("Each reviewer must be different."));
                }
                text = text.replace(
                    &format!("{{{name}}}"),
                    &arg.as_str()
                        .map(str::to_owned)
                        .unwrap_or_else(|| arg.to_string()),
                );
            }
            rendered.push(json!({"command_id":command["id"],"command":text,"reason":r["reason"],"evidence_event_ids":r["evidence_event_ids"]}));
        }
        if !rendered.is_empty()
            && (state["quality"]["uncertain"] == true || state["quality"]["truncated"] == true)
        {
            return Err(ApiError::new(
                409,
                "context_uncertain",
                "Resolve missing packets, pending commands, callbacks or transport uncertainty first.",
            ));
        }
        let existing: Option<String> = tx
            .query_row(
                "SELECT document FROM advice WHERE advice_id=?",
                [advice["advice_id"].as_str().unwrap()],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(existing) = existing {
            let mut saved = decoded(existing);
            if advice
                .as_object()
                .unwrap()
                .iter()
                .any(|(k, v)| &saved[k] != v)
            {
                return Err(ApiError::new(
                    409,
                    "advice_conflict",
                    "advice_id already exists with different content.",
                ));
            }
            saved["stale"] = false.into();
            return Ok((200, saved));
        }
        let mut record = advice.clone();
        record["created_at"] = now().into();
        record["stale"] = false.into();
        record["rendered_recommendations"] = rendered.into();
        tx.execute(
            "INSERT INTO advice VALUES (?,?,?,?)",
            params![
                advice["advice_id"].as_str(),
                advice["stream_id"].as_str(),
                record.to_string(),
                record["created_at"].as_str()
            ],
        )?;
        tx.commit()?;
        Ok((201, record))
    }
    pub fn examples(&self) -> Value {
        let text =
            std::fs::read_to_string(self.root.join("docs/adviser-events.jsonl")).expect("examples");
        json!({"events":text.lines().filter(|l| !l.trim().is_empty()).map(|l| serde_json::from_str::<Value>(l).unwrap()).collect::<Vec<_>>(),"advice":read_json(&self.root.join("docs/adviser-demo-advice.json"))})
    }
}
fn snapshot(db: &Connection, policy: &Value, stream: Option<&str>) -> Result<Value> {
    let streams = db.prepare("SELECT s.*, COUNT(e.event_id), MAX(e.sequence) FROM streams s JOIN events e USING(stream_id) GROUP BY s.stream_id ORDER BY MAX(e.rowid) DESC")?.query_map([],|r| Ok(json!({"stream_id":r.get::<_,String>(0)?,"game":decoded(r.get(1)?),"revision":r.get::<_,i64>(2)?,"last_seen_at":r.get::<_,String>(3)?,"event_count":r.get::<_,i64>(4)?,"last_sequence":r.get::<_,i64>(5)?})))?.collect::<std::result::Result<Vec<_>,_>>()?;
    let selected = stream.or_else(|| streams.first().and_then(|s| s["stream_id"].as_str()));
    let current = streams.iter().find(|s| s["stream_id"].as_str() == selected);
    if selected.is_some() && current.is_none() {
        return Err(ApiError::new(404, "stream_not_found", "Unknown stream_id."));
    }
    let revision = current.map_or(0, |s| s["revision"].as_i64().unwrap());
    let events = db
        .prepare("SELECT document FROM events WHERE stream_id=? ORDER BY sequence")?
        .query_map([selected], |r| Ok(decoded(r.get(0)?)))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let advice = db
        .prepare("SELECT document FROM advice WHERE stream_id=? ORDER BY rowid DESC")?
        .query_map([selected], |r| {
            let mut a = decoded(r.get(0)?);
            a["stale"] = (a["based_on_revision"] != revision
                || a["policy_version"] != policy["policy_version"])
                .into();
            Ok(a)
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(
        json!({"api_version":"2","streams":streams,"selected_stream_id":selected,"revision":revision,"events":events,"advice":advice,"policy":policy,"quality":quality(&events)}),
    )
}
