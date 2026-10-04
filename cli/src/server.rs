//! Loopback HTTP API with bounded bodies and concurrent, cancellable plugin calls.
use crate::composition::App;
use axum::{
    body::{to_bytes, Body},
    extract::{Request, State},
    http::{header, StatusCode},
    response::Response,
    routing::any,
    Router,
};
use nctool_plugin_sdk::*;
use std::{
    net::{IpAddr, SocketAddr},
    sync::Arc,
    time::Duration,
};
use tokio::sync::Semaphore;
mod embedded {
    include!("../ui/assets.rs");
}
const HTML: &str = include_str!("../ui/index.html");
#[derive(Clone)]
struct WebState {
    app: Arc<App>,
    address: SocketAddr,
    slots: Arc<Semaphore>,
}
fn response(status: StatusCode, value: Value) -> Response {
    let mut response = Response::new(Body::from(value.to_string()));
    *response.status_mut() = status;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        "application/json; charset=utf-8".parse().unwrap(),
    );
    secure_headers(&mut response);
    response
}
fn secure_headers(response: &mut Response) {
    for(key,value)in [("x-content-type-options","nosniff"),("x-frame-options","DENY"),("cache-control","no-store"),("referrer-policy","no-referrer"),("content-security-policy","default-src 'self'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; connect-src 'self'; frame-ancestors 'none'; object-src 'none'; base-uri 'none'")]{response.headers_mut().insert(axum::http::HeaderName::from_static(key),value.parse().unwrap());}
}
fn error(error: PluginError) -> Response {
    let status = match error.code.as_str() {
        "not_found" | "action_not_found" | "not_found_asset" => StatusCode::NOT_FOUND,
        "conflict" | "duplicate_request" => StatusCode::CONFLICT,
        "plugin_busy" => StatusCode::SERVICE_UNAVAILABLE,
        "plugin_timeout" => StatusCode::GATEWAY_TIMEOUT,
        "plugin_exited" | "rpc_protocol" | "invalid_output" => StatusCode::BAD_GATEWAY,
        "cancelled" => StatusCode::REQUEST_TIMEOUT,
        _ => StatusCode::BAD_REQUEST,
    };
    response(status, json!({"ok":false,"error":error}))
}
async fn handle(State(state): State<WebState>, request: Request) -> Response {
    let host = request
        .headers()
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let hosts = [
        state.address.to_string(),
        format!("localhost:{}", state.address.port()),
    ];
    if !hosts.iter().any(|h| h == host) {
        return error(PluginError::new("invalid_host", "unexpected Host header"));
    }
    if request
        .headers()
        .get("sec-fetch-site")
        .and_then(|v| v.to_str().ok())
        == Some("cross-site")
    {
        return error(PluginError::new("cross_site", "cross-site requests denied"));
    }
    if let Some(origin) = request.headers().get(header::ORIGIN) {
        if origin.to_str().ok() != Some(format!("http://{host}").as_str()) {
            return error(PluginError::new(
                "cross_site",
                "Origin does not match local host",
            ));
        }
    }
    let method = request.method().as_str().to_owned();
    let path = request.uri().path().to_owned();
    if method == "GET" && path == "/" {
        let mut response = Response::new(Body::from(HTML));
        response.headers_mut().insert(
            header::CONTENT_TYPE,
            "text/html; charset=utf-8".parse().unwrap(),
        );
        secure_headers(&mut response);
        return response;
    }
    if method == "GET" {
        if let Some((_, mime, bytes)) = embedded::FILES.iter().find(|(asset, _, _)| *asset == path)
        {
            let mut response = Response::new(Body::from(*bytes));
            response
                .headers_mut()
                .insert(header::CONTENT_TYPE, mime.parse().unwrap());
            secure_headers(&mut response);
            return response;
        }
    }
    if !path.starts_with("/api/v2/") {
        return error(PluginError::new("not_found", path));
    }
    if method == "POST"
        && !request
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|s| s.split(';').next() == Some("application/json"))
    {
        return error(PluginError::new(
            "content_type",
            "POST requires application/json",
        ));
    }
    let bytes = match tokio::time::timeout(
        Duration::from_secs(10),
        to_bytes(
            request.into_body(),
            if path.starts_with("/api/v2/bundle/") {
                64 * 1024 * 1024
            } else {
                1024 * 1024
            },
        ),
    )
    .await
    {
        Ok(Ok(b)) => b,
        Ok(Err(_)) => {
            return response(
                StatusCode::PAYLOAD_TOO_LARGE,
                json!({"ok":false,"error":{"code":"body_limit","message":"request body exceeds 1 MiB"}}),
            )
        }
        Err(_) => {
            return error(PluginError::new(
                "body_timeout",
                "request body read timed out",
            ))
        }
    };
    let body = if bytes.is_empty() {
        json!({})
    } else {
        match parse_json(&bytes) {
            Ok(v) => v,
            Err(e) => return error(e),
        }
    };
    if path.starts_with("/api/v2/cancel/") {
        return match state.app.dispatch(&method, &path, body) {
            Ok(data) => response(StatusCode::OK, json!({"ok":true,"data":data})),
            Err(e) => error(e),
        };
    }
    let permit = match state.slots.clone().try_acquire_owned() {
        Ok(p) => p,
        Err(_) => {
            return error(PluginError::new(
                "plugin_busy",
                "host concurrency limit reached",
            ))
        }
    };
    match tokio::task::spawn_blocking(move || {
        let _permit = permit;
        state.app.dispatch(&method, &path, body)
    })
    .await
    {
        Ok(Ok(data)) => response(StatusCode::OK, json!({"ok":true,"data":data})),
        Ok(Err(e)) => error(e),
        Err(e) => error(PluginError::new("host_error", e.to_string())),
    }
}
pub async fn serve(app: Arc<App>, host: IpAddr, port: u16) -> PluginResult<()> {
    if !host.is_loopback() {
        return Err(PluginError::new(
            "bind_address",
            "WebUI only binds loopback addresses",
        ));
    }
    let listener = tokio::net::TcpListener::bind(SocketAddr::new(host, port)).await?;
    let address = listener.local_addr()?;
    eprintln!("NCtool {} profile: http://{address}", app.profile);
    let state = WebState {
        app: app.clone(),
        address,
        slots: Arc::new(Semaphore::new(8)),
    };
    let router = Router::new()
        .route("/", any(handle))
        .route("/{*path}", any(handle))
        .with_state(state);
    let stopping = app.runtime.clone();
    axum::serve(listener, router)
        .with_graceful_shutdown(async move {
            let _ = tokio::signal::ctrl_c().await;
            stopping.shutdown();
        })
        .await?;
    app.runtime.shutdown();
    Ok(())
}
