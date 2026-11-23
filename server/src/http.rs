//! Answers and errors. A banned sender gets `ApiError::Silent`: a 200 with an empty body, the same
//! as a stored write, so it cannot tell it was dropped.

use std::net::{IpAddr, SocketAddr};

use std::convert::Infallible;

use axum::extract::{ConnectInfo, FromRequestParts};
use axum::http::request::Parts;
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;

use crate::state::AppState;

/// The TCP peer's address, when the server has it (`into_make_service_with_connect_info`). Absent in
/// the tests, which send requests without a socket.
pub struct Peer(pub Option<SocketAddr>);

impl<S: Send + Sync> FromRequestParts<S> for Peer {
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        Ok(Peer(parts.extensions.get::<ConnectInfo<SocketAddr>>().map(|c| c.0)))
    }
}

#[derive(Debug)]
pub enum ApiError {
    /// A banned sender: 200 and nothing stored.
    Silent,
    BadRequest(String),
    Unauthorized(&'static str),
    NotFound(&'static str),
    Conflict(String),
    TooLarge(String),
    /// The stored files are over the storage budget (507, no new uploads until space frees).
    Full(String),
    /// A 429: the seconds until the window frees (or the ban ends), sent as `Retry-After`.
    Limited(u64),
    Internal(String),
}

impl From<rusqlite::Error> for ApiError {
    fn from(e: rusqlite::Error) -> Self {
        ApiError::Internal(format!("sqlite: {e}"))
    }
}

fn error_body(status: u16, msg: impl Into<String>) -> Response {
    let code = StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    (code, Json(serde_json::json!({ "error": msg.into() }))).into_response()
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        match self {
            ApiError::Silent => Json(serde_json::json!({})).into_response(),
            ApiError::BadRequest(m) => error_body(400, m),
            ApiError::Unauthorized(m) => error_body(401, m),
            ApiError::NotFound(m) => error_body(404, m),
            ApiError::Conflict(m) => error_body(409, m),
            ApiError::TooLarge(m) => error_body(413, m),
            ApiError::Full(m) => error_body(507, m),
            ApiError::Limited(secs) => {
                let mut r = error_body(429, "rate limit");
                // At least one second: a zero would say "retry now" to a client that is still refused.
                let secs = secs.max(1);
                r.headers_mut().insert(header::RETRY_AFTER, secs.to_string().parse().expect("a header value"));
                r
            }
            ApiError::Internal(m) => {
                tracing::error!("internal: {m}");
                error_body(500, "internal error")
            }
        }
    }
}

pub fn json<T: Serialize>(value: &T) -> Response {
    Json(value).into_response()
}

/// The client's address: the peer's, or `X-Real-IP` behind nginx (`trust_proxy`). The address is
/// used in memory for the rate limits and hashed for the bans; it is never stored.
pub fn client_ip(st: &AppState, headers: &HeaderMap, peer: Option<SocketAddr>) -> IpAddr {
    if st.cfg.trust_proxy {
        if let Some(ip) = headers
            .get("x-real-ip")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.trim().parse::<IpAddr>().ok())
        {
            return ip;
        }
    }
    peer.map(|p| p.ip()).unwrap_or(IpAddr::from([0, 0, 0, 0]))
}

/// The hook for a country lookup (the analytics spec's option: a country computed from the address, the
/// address dropped at once). Not built: no database ships with the server, so it answers `None`.
pub fn country_of(_ip: IpAddr) -> Option<&'static str> {
    None
}

/// A release version: `X.Y.Z` with digits (at most four each), or one of the configured list.
pub fn release_ok(version: &str, allowed: &[String]) -> bool {
    if !allowed.is_empty() {
        return allowed.iter().any(|a| a == version);
    }
    let parts: Vec<&str> = version.split('.').collect();
    parts.len() == 3
        && parts.iter().all(|p| !p.is_empty() && p.len() <= 4 && p.bytes().all(|b| b.is_ascii_digit()))
}

/// The state of the body-size caps, as a refusal the handlers share.
pub fn too_large(max: usize, len: usize) -> ApiError {
    ApiError::TooLarge(format!("body of {len} bytes is over the limit {max}"))
}
