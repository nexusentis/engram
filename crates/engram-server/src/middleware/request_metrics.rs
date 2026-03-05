//! Tower middleware that records per-request Prometheus metrics.

use std::time::Instant;

use axum::body::Body;
use axum::extract::MatchedPath;
use axum::http::Request;
use axum::middleware::Next;
use axum::response::Response;
use engram_core::api::METRICS;

pub async fn request_metrics(req: Request<Body>, next: Next) -> Response {
    let method = req.method().to_string();
    let path = req
        .extensions()
        .get::<MatchedPath>()
        .map(|p| p.as_str().to_string())
        .unwrap_or_else(|| req.uri().path().to_string());

    let start = Instant::now();
    let response = next.run(req).await;
    let duration = start.elapsed().as_secs_f64();
    let status = response.status().as_u16();

    METRICS.record_request(&method, &path, status, duration);

    response
}
