use crate::{
    api::{Reply, Service},
    domain::*,
};
use axum::{
    Router,
    body::to_bytes,
    extract::{Path, Request, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::post,
};
use std::net::SocketAddr;
pub const MAX_REQUEST_BYTES: usize = 8 * 1024 * 1024;
pub fn router(service: Service) -> Router {
    Router::new()
        .route("/v1/{operation}", post(handle))
        .with_state(service)
}
fn response(reply: Reply) -> Response {
    let status = StatusCode::from_u16(reply.status()).expect("static status");
    let mut response = (status, axum::Json(reply)).into_response();
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("no-store"),
    );
    if status == StatusCode::UNAUTHORIZED {
        response.headers_mut().insert(
            header::WWW_AUTHENTICATE,
            header::HeaderValue::from_static("Bearer"),
        );
    }
    response
}
async fn handle(
    State(service): State<Service>,
    Path(operation): Path<String>,
    request: Request,
) -> Response {
    let headers = request.headers();
    // Bearer is the only authentication mechanism; no role/client identity headers.
    let token = if headers.get_all(header::AUTHORIZATION).iter().count() == 1 {
        headers
            .get(header::AUTHORIZATION)
            .and_then(|h| h.to_str().ok())
            .and_then(|h| h.strip_prefix("Bearer "))
            .map(str::to_owned)
    } else {
        None
    };
    let Some(token) = token else {
        return response(Reply::error(Error::Unauthenticated));
    };
    let auth_service = service.clone();
    let principal =
        match tokio::task::spawn_blocking(move || auth_service.authenticate(&token)).await {
            Ok(Ok(p)) => p,
            Ok(Err(e)) => return response(Reply::error(e)),
            Err(_) => return response(Reply::error(Error::Credentials)),
        };
    // This local API is not a browser endpoint. Reject Origin, including null.
    if headers.contains_key(header::ORIGIN) {
        return response(Reply::error(Error::Forbidden));
    }
    let bytes = match to_bytes(request.into_body(), MAX_REQUEST_BYTES).await {
        Ok(b) => b,
        Err(_) => return response(Reply::error(invalid("request", "too large"))),
    };
    let value = match serde_json::from_slice(&bytes) {
        Ok(v) => v,
        Err(_) => return response(Reply::error(invalid("request", "invalid JSON"))),
    };
    match tokio::task::spawn_blocking(move || service.dispatch(&principal, &operation, value)).await
    {
        Ok(reply) => response(reply),
        Err(_) => response(Reply::error(Error::IntegrityFailure(
            "worker failed".into(),
        ))),
    }
}
pub async fn bind(address: SocketAddr) -> Result<tokio::net::TcpListener> {
    if !address.ip().is_loopback() {
        return Err(invalid("bind", "loopback required"));
    }
    Ok(tokio::net::TcpListener::bind(address).await?)
}
