//! X-Request-Id middleware.
//!
//! Reads `X-Request-Id` from the incoming request (if present and valid),
//! otherwise generates a UUID v7. Sets it on the response header and inserts
//! it into request extensions for downstream use.

use axum::extract::Request;
use axum::http::HeaderValue;
use axum::middleware::Next;
use axum::response::Response;
use uuid::Uuid;

/// A request identifier stored in request extensions.
#[derive(Clone)]
pub struct RequestId(pub String);

/// Middleware that propagates or generates an `X-Request-Id` header.
///
/// Incoming IDs are sanitised: truncated to 128 chars, must be ASCII graphic
/// only (no control chars or spaces) to prevent log injection.
pub async fn request_id(mut req: Request, next: Next) -> Response {
    let id = req
        .headers()
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .filter(|s| s.len() <= 128 && s.chars().all(|c| c.is_ascii_graphic()))
        .map(String::from)
        .unwrap_or_else(|| Uuid::now_v7().to_string());

    req.extensions_mut().insert(RequestId(id.clone()));

    let mut resp = next.run(req).await;
    if let Ok(v) = HeaderValue::from_str(&id) {
        resp.headers_mut().insert("x-request-id", v);
    }
    resp
}
