use ameise_adviser::{project_root, runner::Runner};
use std::{collections::HashSet, time::Duration};
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut base = "http://127.0.0.1:8765".to_owned();
    let mut poll = 2u64;
    let mut quiet = 1u64;
    let mut timeout = 180u64;
    let mut args = std::env::args().skip(1);
    let mut command = Vec::new();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--url" => base = args.next().ok_or("--url needs a value")?,
            "--poll-seconds" => {
                poll = args.next().ok_or("--poll-seconds needs a value")?.parse()?
            }
            "--quiet-seconds" => {
                quiet = args
                    .next()
                    .ok_or("--quiet-seconds needs a value")?
                    .parse()?
            }
            "--model-timeout" => {
                timeout = args
                    .next()
                    .ok_or("--model-timeout needs a value")?
                    .parse()?
            }
            "--" => {
                command.extend(args);
                break;
            }
            "--help" | "-h" => {
                println!(
                    "Usage: adviser-runner [--url URL] [--poll-seconds 2] [--quiet-seconds 1] [--model-timeout 180] -- MODEL_ADAPTER [ARGS...]\nAdapter reads one JSON request on stdin and writes one complete advice object on stdout. Ctrl-C stops polling and the active adapter."
                );
                return Ok(());
            }
            _ => return Err(format!("Unknown option: {arg}").into()),
        }
    }
    if command.is_empty() {
        return Err("Supply a model adapter executable after -- (see --help)".into());
    }
    if poll == 0 || timeout == 0 {
        return Err("Poll interval and model timeout must be positive".into());
    }
    let url = url::Url::parse(&base)?;
    if url.scheme() != "http"
        || !matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "[::1]"))
        || !url.username().is_empty()
        || url.password().is_some()
        || url.path() != "/"
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("Companion URL must be a loopback HTTP origin".into());
    }
    let root = project_root();
    let mut runner = Runner {
        client: reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .build()?,
        base: base.trim_end_matches('/').into(),
        command,
        instructions: std::fs::read_to_string(root.join("adviser/AGENT_GUIDELINES.md"))?,
        schema: serde_json::from_slice(&std::fs::read(
            root.join("docs/adviser-advice.schema.json"),
        )?)?,
        rules: std::fs::read_to_string(root.join("docs/README.md"))?,
        quiet: Duration::from_secs(quiet),
        model_timeout: Duration::from_secs(timeout),
        seen: HashSet::new(),
        pending: None,
    };
    let work = async {
        let mut backoff = poll;
        loop {
            match runner.tick().await {
                Ok(()) => backoff = poll,
                Err(e) => {
                    eprintln!("{e}");
                    backoff = backoff.saturating_mul(2).min(60).max(poll);
                }
            }
            tokio::time::sleep(Duration::from_secs(backoff)).await;
        }
    };
    tokio::select! { _=work=>{}, _=tokio::signal::ctrl_c()=>{eprintln!("Adviser runner stopped");} }
    Ok(())
}
