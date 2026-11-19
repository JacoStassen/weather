//! Owner token for servers reachable from the internet.
//!
//! Locally (no `OWNER_TOKEN`) every request counts as the owner, as before.
//! With a token set:
//! - writing custom sources, testing them and MCP need `Authorization: Bearer <token>`;
//! - everyone else gets the dashboard with the built-in providers only, so
//!   visitors can't spend the owner's API keys through a custom source.

use std::sync::Arc;

use axum::{
    extract::{Request, State},
    http::{header, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

/// Shortest token accepted, so a weak one can't be set by accident.
const MIN_TOKEN_LEN: usize = 24;

#[derive(Clone, Default)]
pub struct OwnerToken(Option<Arc<str>>);

impl OwnerToken {
    pub fn from_env() -> anyhow::Result<Self> {
        match std::env::var("OWNER_TOKEN").ok().map(|t| t.trim().to_owned()) {
            None => Ok(Self(None)),
            Some(t) if t.is_empty() => Ok(Self(None)),
            Some(t) if t.len() < MIN_TOKEN_LEN => {
                anyhow::bail!("OWNER_TOKEN must be at least {MIN_TOKEN_LEN} characters (try: openssl rand -hex 24)")
            }
            Some(t) => Ok(Self(Some(t.into()))),
        }
    }

    #[cfg(test)]
    pub fn new(token: &str) -> Self {
        Self(Some(token.into()))
    }

    pub fn is_set(&self) -> bool {
        self.0.is_some()
    }

    /// `None`: no credentials sent. `Some(false)`: wrong ones.
    fn check(&self, req: &Request) -> Option<bool> {
        let sent = req
            .headers()
            .get(header::AUTHORIZATION)?
            .to_str()
            .ok()
            .and_then(|v| v.strip_prefix("Bearer "))
            .map(str::trim);
        Some(match (&self.0, sent) {
            (Some(expected), Some(sent)) => constant_time_eq(expected.as_bytes(), sent.as_bytes()),
            _ => false,
        })
    }
}

/// What the caller may do. Set on every request by [`identify`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Access {
    pub owner: bool,
}

impl Access {
    pub const OWNER: Self = Self { owner: true };
    pub const VISITOR: Self = Self { owner: false };

    /// Custom sources hold the owner's API keys.
    pub fn may_use(&self, provider_id: &str) -> bool {
        self.owner || !provider_id.starts_with("custom:")
    }
}

fn unauthorized(message: &str) -> Response {
    let mut res = (
        StatusCode::UNAUTHORIZED,
        Json(json!({ "code": "unauthorized", "message": message })),
    )
        .into_response();
    res.headers_mut()
        .insert(header::WWW_AUTHENTICATE, header::HeaderValue::from_static("Bearer"));
    res
}

/// Works out [`Access`] for the request. A wrong token is rejected outright
/// rather than downgraded to visitor, so a mistyped token is noticed.
pub async fn identify(State(token): State<OwnerToken>, mut req: Request, next: Next) -> Response {
    let access = if !token.is_set() {
        Access::OWNER
    } else {
        match token.check(&req) {
            None => Access::VISITOR,
            Some(true) => Access::OWNER,
            Some(false) => return unauthorized("wrong access token"),
        }
    };
    req.extensions_mut().insert(access);
    next.run(req).await
}

/// For routes only the owner may use. Runs after [`identify`].
pub async fn require_owner(req: Request, next: Next) -> Response {
    match req.extensions().get::<Access>() {
        Some(a) if a.owner => next.run(req).await,
        _ => unauthorized("this needs the server's access token"),
    }
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_tokens() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"abcd"));
    }

    #[test]
    fn visitors_cannot_use_custom_sources() {
        assert!(Access::VISITOR.may_use("open-meteo"));
        assert!(!Access::VISITOR.may_use("custom:owm"));
        assert!(Access::OWNER.may_use("custom:owm"));
    }
}
