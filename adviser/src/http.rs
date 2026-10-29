use crate::{ApiError, Result, Store, validation};
use axum::{
    Router,
    body::{Body, to_bytes},
    extract::{Request, State},
    http::{Method, StatusCode},
    response::{IntoResponse, Response},
};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
const MAX_BODY: usize = 1024 * 1024;
#[derive(Clone)]
pub struct Service {
    pub store: Arc<Mutex<Store>>,
    pub port: u16,
}
pub fn router(store: Store, port: u16) -> Router {
    Router::new().fallback(handle).with_state(Service {
        store: Arc::new(Mutex::new(store)),
        port,
    })
}
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        response(
            self.status,
            json!({"error":{"code":self.code,"message":self.message}})
                .to_string()
                .into_bytes(),
            "application/json; charset=utf-8",
        )
    }
}
fn response(status: u16, body: Vec<u8>, mime: &str) -> Response {
    Response::builder().status(StatusCode::from_u16(status).unwrap())
        .header("Content-Type",mime).header("Content-Length",body.len())
        .header("Cache-Control","no-store").header("X-Content-Type-Options","nosniff")
        .header("Referrer-Policy","no-referrer")
        .header("Content-Security-Policy","default-src 'self'; script-src 'self'; style-src 'self'; connect-src 'self'; img-src 'self' data:; object-src 'none'; base-uri 'none'; frame-ancestors 'none'; form-action 'self'")
        .body(Body::from(body)).unwrap()
}
fn local(value: &str, port: u16, origin: bool) -> bool {
    let text = if origin {
        value.to_owned()
    } else {
        format!("http://{value}")
    };
    let Ok(u) = url::Url::parse(&text) else {
        return false;
    };
    u.scheme() == "http"
        && matches!(u.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"))
        && u.port_or_known_default() == Some(port)
        && u.username().is_empty()
        && u.password().is_none()
        && u.query().is_none()
        && u.fragment().is_none()
        && u.path() == "/"
        && !value.ends_with('/')
}
async fn handle(State(service): State<Service>, req: Request) -> Response {
    let head = req.method() == Method::HEAD;
    let mut out = match route(service, req).await {
        Ok(r) => r,
        Err(e) => e.into_response(),
    };
    if head {
        *out.body_mut() = Body::empty();
    }
    out
}
async fn route(service: Service, req: Request) -> Result<Response> {
    let bad = |status, code, msg| ApiError::new(status, code, msg);
    let headers = req.headers();
    if !headers
        .get("host")
        .and_then(|s| s.to_str().ok())
        .is_some_and(|v| local(v, service.port, false))
    {
        return Err(bad(
            400,
            "invalid_host",
            "Use the local loopback host and port.",
        ));
    }
    if headers
        .get("origin")
        .is_some_and(|s| !s.to_str().is_ok_and(|v| local(v, service.port, true)))
    {
        return Err(bad(
            403,
            "foreign_origin",
            "Only this service's loopback browser origin is allowed.",
        ));
    }
    let path = percent_encoding::percent_decode_str(req.uri().path())
        .decode_utf8_lossy()
        .to_string();
    let method = req.method().clone();
    if method == Method::GET || method == Method::HEAD {
        let mut store = service.store.lock().unwrap();
        let value: Option<Value> = match path.as_str() {
            "/api/v2/health" => {
                Some(json!({"status":"ok","mode":"adviser","simulator_control":false}))
            }
            "/api/v2/policy" => Some(store.policy.clone()),
            "/api/v2/examples" => Some(store.examples()),
            "/api/v2/state" | "/api/v2/context" => {
                let query = url::form_urlencoded::parse(req.uri().query().unwrap_or("").as_bytes())
                    .collect::<Vec<_>>();
                if query.len() > 1 || query.iter().any(|(k, v)| k != "stream_id" || v.is_empty()) {
                    return Err(bad(
                        400,
                        "invalid_query",
                        "Only one nonempty stream_id parameter is supported.",
                    ));
                }
                Some(store.snapshot(
                    query.first().map(|(_, v)| v.as_ref()),
                    path.ends_with("context"),
                )?)
            }
            _ => None,
        };
        if let Some(value) = value {
            return Ok(response(
                200,
                value.to_string().into_bytes(),
                "application/json; charset=utf-8",
            ));
        }
        let docs = [
            "README.md",
            "AMAISE_commandlist-qa200-en-v1.95.pdf",
            "json-api-integration.md",
            "adviser-command-policy.json",
            "adviser-event.schema.json",
            "adviser-advice.schema.json",
            "adviser-events.jsonl",
            "adviser-demo-advice.json",
        ];
        let file = if path
            .strip_prefix("/docs/")
            .is_some_and(|p| docs.contains(&p))
        {
            store.root.join(&path[1..])
        } else if ["/", "/index.html", "/app.js", "/theme.js", "/styles.css"]
            .contains(&path.as_str())
        {
            store.root.join("adviser/dist").join(if path == "/" {
                "index.html"
            } else {
                &path[1..]
            })
        } else {
            return Err(bad(404, "not_found", "No such resource."));
        };
        let mime = match file.extension().and_then(|s| s.to_str()).unwrap_or("") {
            "html" => "text/html; charset=utf-8",
            "js" => "text/javascript; charset=utf-8",
            "css" => "text/css; charset=utf-8",
            "json" => "application/json; charset=utf-8",
            "md" => "text/markdown; charset=utf-8",
            "pdf" => "application/pdf",
            _ => "application/octet-stream",
        };
        return Ok(response(
            200,
            std::fs::read(file).map_err(|_| bad(404, "not_found", "Resource unavailable."))?,
            mime,
        ));
    }
    if method != Method::POST || !["/api/v2/events", "/api/v2/advice"].contains(&path.as_str()) {
        return Err(bad(
            404,
            "not_found",
            "No such endpoint. This adviser has no simulator control API.",
        ));
    }
    if headers.contains_key("transfer-encoding") {
        return Err(bad(
            400,
            "invalid_body",
            "Send Content-Length, not Transfer-Encoding.",
        ));
    }
    if !headers
        .get("content-type")
        .and_then(|s| s.to_str().ok())
        .is_some_and(|s| {
            s.split(';')
                .next()
                .unwrap_or("")
                .trim()
                .eq_ignore_ascii_case("application/json")
        })
    {
        return Err(bad(
            400,
            "invalid_content_type",
            "Use Content-Type: application/json.",
        ));
    }
    let len = headers
        .get("content-length")
        .and_then(|s| s.to_str().ok())
        .and_then(|s| s.parse::<usize>().ok())
        .filter(|n| *n > 0)
        .ok_or_else(|| bad(400, "invalid_body", "A valid Content-Length is required."))?;
    if len > MAX_BODY {
        return Err(bad(
            413,
            "body_too_large",
            "JSON body exceeds the 1 MiB limit.",
        ));
    }
    let bytes = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        to_bytes(req.into_body(), MAX_BODY),
    )
    .await
    .map_err(|_| bad(400, "invalid_body", "Request body timed out."))?
    .map_err(|_| bad(400, "invalid_body", "Incomplete request body."))?;
    if bytes.len() != len {
        return Err(bad(400, "invalid_body", "Incomplete request body."));
    }
    let body = validation::parse(&bytes)?;
    let mut store = service.store.lock().unwrap();
    let (status, value) = if path.ends_with("events") {
        store.ingest(&body)?
    } else {
        store.submit(&body)?
    };
    Ok(response(
        status,
        value.to_string().into_bytes(),
        "application/json; charset=utf-8",
    ))
}
