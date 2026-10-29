use ameise_adviser::{Store, http, project_root};
use std::{net::IpAddr, path::PathBuf};
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut host = "127.0.0.1".to_owned();
    let mut port = 8765u16;
    let mut data = project_root().join("adviser/.runtime");
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--host" => host = args.next().ok_or("--host needs a value")?,
            "--port" => port = args.next().ok_or("--port needs a value")?.parse()?,
            "--data-dir" => data = PathBuf::from(args.next().ok_or("--data-dir needs a value")?),
            "--help" | "-h" => {
                println!(
                    "AMEISE observer receiver\nUsage: ameise-adviser [--host 127.0.0.1|localhost|::1] [--port 8765] [--data-dir PATH]"
                );
                return Ok(());
            }
            _ => return Err(format!("Unknown option: {arg}").into()),
        }
    }
    if !["127.0.0.1", "localhost", "::1"].contains(&host.as_str()) {
        return Err("Only loopback bind addresses are allowed".into());
    }
    let ip: IpAddr = if host == "localhost" {
        "127.0.0.1"
    } else {
        &host
    }
    .parse()?;
    let listener = tokio::net::TcpListener::bind((ip, port)).await?;
    let address = listener.local_addr()?;
    let store = Store::open(&data, &project_root())?;
    println!("AMEISE adviser: http://{address} (observation only)");
    axum::serve(listener, http::router(store, address.port()))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
