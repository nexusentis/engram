//! Bearer token authentication middleware using engram_core::api::auth.

use axum::extract::Request;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use tower::{Layer, Service};

use engram_core::api::{authenticate, AuthConfig, ErrorResponse};

/// Tower Layer that enforces bearer-token auth.
#[derive(Clone)]
pub struct AuthLayer {
    config: AuthConfig,
}

impl AuthLayer {
    pub fn new(config: AuthConfig) -> Self {
        Self { config }
    }
}

impl<S> Layer<S> for AuthLayer {
    type Service = AuthMiddleware<S>;

    fn layer(&self, inner: S) -> Self::Service {
        AuthMiddleware {
            inner,
            config: self.config.clone(),
        }
    }
}

/// The middleware service produced by `AuthLayer`.
#[derive(Clone)]
pub struct AuthMiddleware<S> {
    inner: S,
    config: AuthConfig,
}

impl<S> Service<Request> for AuthMiddleware<S>
where
    S: Service<Request, Response = Response> + Send + Clone + 'static,
    S::Future: Send + 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Self::Response, Self::Error>> + Send>,
    >;

    fn poll_ready(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: Request) -> Self::Future {
        let config = self.config.clone();
        let mut inner = self.inner.clone();

        Box::pin(async move {
            let path = req.uri().path().to_string();
            let auth_header = req
                .headers()
                .get(axum::http::header::AUTHORIZATION)
                .and_then(|v| v.to_str().ok())
                .map(String::from);

            match authenticate(&config, &path, auth_header.as_deref()) {
                Ok(state) => {
                    req.extensions_mut().insert(state);
                    inner.call(req).await
                }
                Err(auth_err) => {
                    tracing::debug!(path = %path, code = %auth_err.code, "Auth rejected");
                    let body = ErrorResponse::new(auth_err.error, auth_err.code);
                    Ok((
                        StatusCode::UNAUTHORIZED,
                        [(axum::http::header::WWW_AUTHENTICATE, "Bearer")],
                        Json(body),
                    )
                        .into_response())
                }
            }
        })
    }
}
