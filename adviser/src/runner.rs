//! A model adapter is an executable: one JSON request on stdin, one advice object on stdout.
//! It receives no simulator connection or execution capability from this process.
use crate::{ApiError, Result, validation};
use serde_json::{Value, json};
use std::{collections::HashSet, process::Stdio, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::Command,
};
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Revision(pub String, pub i64, pub String);
pub fn revision(context: &Value) -> Option<Revision> {
    Some(Revision(
        context["selected_stream_id"].as_str()?.into(),
        context["revision"].as_i64()?,
        context["policy_version"].as_str()?.into(),
    ))
}
pub fn reviewed(context: &Value) -> bool {
    context["advice"].as_array().is_some_and(|a| {
        a.iter().any(|a| {
            a["source"] == "agent"
                && a["based_on_revision"] == context["revision"]
                && a["policy_version"] == context["policy_version"]
        })
    })
}
pub struct Runner {
    pub client: reqwest::Client,
    pub base: String,
    pub command: Vec<String>,
    pub instructions: String,
    pub schema: Value,
    pub rules: String,
    pub quiet: Duration,
    pub model_timeout: Duration,
    pub seen: HashSet<Revision>,
    pub pending: Option<Value>,
}
impl Runner {
    async fn context(&self, stream: Option<&str>) -> Result<Value> {
        let mut url = url::Url::parse(&format!("{}/api/v2/context", self.base))
            .expect("validated companion URL");
        if let Some(s) = stream {
            url.query_pairs_mut().append_pair("stream_id", s);
        }
        let request = self.client.get(url);
        let response = request
            .send()
            .await
            .map_err(|_| ApiError::new(503, "unavailable", "Companion unavailable"))?;
        if !response.status().is_success() {
            return Err(ApiError::new(503, "unavailable", "Cannot read context"));
        }
        response
            .json()
            .await
            .map_err(|_| ApiError::new(503, "invalid_context", "Invalid context response"))
    }
    async fn post_pending(&mut self) -> Result<()> {
        let Some(body) = self.pending.as_ref() else {
            return Ok(());
        };
        let latest = self.context(body["stream_id"].as_str()).await?;
        if latest["revision"] != body["based_on_revision"]
            || latest["policy_version"] != body["policy_version"]
            || reviewed(&latest)
        {
            self.pending = None;
            return Ok(());
        }
        // Retain the exact body on network errors, including loss of the response body.
        let response = self
            .client
            .post(format!("{}/api/v2/advice", self.base))
            .json(body)
            .send()
            .await
            .map_err(|_| {
                ApiError::new(
                    503,
                    "unavailable",
                    "Advice delivery uncertain; retaining same ID",
                )
            })?;
        let status = response.status();
        if status.is_server_error() || status.as_u16() == 429 {
            return Err(ApiError::new(
                503,
                "unavailable",
                "Companion temporarily unavailable; retaining the same advice body and ID",
            ));
        }
        let value: Value = response.json().await.map_err(|_| {
            ApiError::new(
                503,
                "unavailable",
                "Advice response lost; retaining same ID",
            )
        })?;
        if status.is_success() {
            self.pending = None;
            return Ok(());
        }
        let code = value["error"]["code"].as_str().unwrap_or("rejected");
        if code == "stale_advice" || code == "policy_conflict" {
            let body = self.pending.take().unwrap();
            self.seen.remove(&Revision(
                body["stream_id"].as_str().unwrap().into(),
                body["based_on_revision"].as_i64().unwrap(),
                body["policy_version"].as_str().unwrap().into(),
            ));
            return Ok(());
        }
        self.pending = None;
        Err(ApiError::new(
            status.as_u16(),
            code,
            "Advice rejected; this revision will not invoke the model again",
        ))
    }
    pub async fn tick(&mut self) -> Result<()> {
        if self.pending.is_some() {
            return self.post_pending().await;
        }
        let context = self.context(None).await?;
        // Visit all streams serially: at most one review in flight, including across streams.
        let streams = context["streams"].as_array().cloned().unwrap_or_default();
        for stream in streams {
            let id = stream["stream_id"].as_str().unwrap();
            let first = self.context(Some(id)).await?;
            let Some(key) = revision(&first) else {
                continue;
            };
            if reviewed(&first) || self.seen.contains(&key) {
                continue;
            }
            tokio::time::sleep(self.quiet).await;
            let current = self.context(Some(id)).await?;
            if revision(&current).as_ref() != Some(&key) || reviewed(&current) {
                continue;
            }
            self.seen.insert(key.clone());
            eprintln!("Reviewing stream {} revision {}", key.0, key.1);
            let input = json!({"instructions":self.instructions,"project_rules":self.rules,"advice_schema":self.schema,"context":current});
            let mut draft = invoke(&self.command, &input, self.model_timeout).await?;
            validation::validate(&draft, &self.schema)?;
            if draft["source"] != "agent"
                || draft["stream_id"] != key.0
                || draft["based_on_revision"] != key.1
                || draft["policy_version"] != key.2
            {
                return Err(ApiError::new(
                    400,
                    "invalid_advice",
                    "Model must preserve reviewed stream, revision, policy and agent source",
                ));
            }
            if (current["quality"]["uncertain"] == true || current["quality"]["truncated"] == true)
                && !draft["recommendations"].as_array().unwrap().is_empty()
            {
                return Err(ApiError::new(
                    400,
                    "context_uncertain",
                    "Model recommended commands from uncertain evidence",
                ));
            }
            // The runner owns transport identity, so a model cannot accidentally reuse an old ID.
            draft["advice_id"] = format!(
                "agent-{}-{}",
                std::process::id(),
                chrono::Utc::now().timestamp_nanos_opt().unwrap()
            )
            .into();
            self.pending = Some(draft);
            return self.post_pending().await;
        }
        Ok(())
    }
}
pub async fn invoke(command: &[String], input: &Value, timeout: Duration) -> Result<Value> {
    let run = async {
        let mut child = Command::new(&command[0])
            .args(&command[1..])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .map_err(|_| ApiError::new(500, "model_failed", "Cannot launch model adapter"))?;
        let mut stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let bytes = input.to_string();
        let write = async move {
            stdin.write_all(bytes.as_bytes()).await?;
            stdin.shutdown().await?;
            drop(stdin);
            Ok::<_, std::io::Error>(())
        };
        let read = async {
            let mut bytes = Vec::new();
            stdout.take(1024 * 1024 + 1).read_to_end(&mut bytes).await?;
            Ok::<_, std::io::Error>(bytes)
        };
        let (_, bytes) = tokio::try_join!(write, read)
            .map_err(|_| ApiError::new(500, "model_failed", "Model adapter I/O failed"))?;
        if bytes.len() > 1024 * 1024 {
            return Err(ApiError::new(
                400,
                "model_failed",
                "Model output exceeds 1 MiB",
            ));
        }
        if !child
            .wait()
            .await
            .map_err(|_| ApiError::new(500, "model_failed", "Model adapter wait failed"))?
            .success()
        {
            return Err(ApiError::new(
                500,
                "model_failed",
                "Model adapter exited unsuccessfully",
            ));
        }
        validation::parse(&bytes)
    };
    tokio::time::timeout(timeout, run)
        .await
        .map_err(|_| ApiError::new(504, "model_timeout", "Model adapter timed out"))?
}
