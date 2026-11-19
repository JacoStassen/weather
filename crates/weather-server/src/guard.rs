//! Protects the local server from other websites open in the user's browser.
//!
//! - Host check: blocks DNS-rebinding, where an attacker's domain resolves to
//!   127.0.0.1 so their page can talk to this server "same-origin".
//! - Origin check: blocks cross-site requests that a browser sends without a
//!   CORS preflight.

use axum::{
    extract::{Request, State},
    http::{header, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

#[derive(Clone, Debug, Default)]
pub struct AllowedHosts {
    /// Extra host names from the `ALLOWED_HOSTS` env var (comma-separated).
    extra: Vec<String>,
}

impl AllowedHosts {
    pub fn from_env() -> Self {
        let extra = std::env::var("ALLOWED_HOSTS")
            .unwrap_or_default()
            .split(',')
            .map(|h| h.trim().to_ascii_lowercase())
            .filter(|h| !h.is_empty())
            .collect();
        Self { extra }
    }

    pub fn extra_hosts(&self) -> &[String] {
        &self.extra
    }

    fn allows(&self, host: &str) -> bool {
        let host = host.to_ascii_lowercase();
        matches!(host.as_str(), "localhost" | "127.0.0.1" | "[::1]" | "::1")
            || host.ends_with(".localhost")
            || self.extra.contains(&host)
    }
}

/// Host part of `host[:port]`, keeping IPv6 brackets.
fn host_of(authority: &str) -> &str {
    if authority.starts_with('[') {
        return authority.find(']').map_or(authority, |i| &authority[..=i]);
    }
    authority.rsplit_once(':').map_or(authority, |(h, _)| h)
}

pub async fn check(State(allowed): State<AllowedHosts>, req: Request, next: Next) -> Response {
    let host_ok = req
        .headers()
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .is_some_and(|h| allowed.allows(host_of(h)));

    // Same-origin GETs often omit Origin; when present it must be ours.
    let origin_ok = match req.headers().get(header::ORIGIN).map(|o| o.to_str()) {
        None => true,
        Some(Ok("null")) | Some(Err(_)) => false,
        Some(Ok(origin)) => origin
            .split_once("://")
            .is_some_and(|(_, authority)| allowed.allows(host_of(authority))),
    };

    if host_ok && origin_ok {
        return next.run(req).await;
    }
    tracing::warn!(host_ok, origin_ok, "blocked request from disallowed host/origin");
    (
        StatusCode::FORBIDDEN,
        Json(json!({ "code": "forbidden_host", "message": "requests must come from localhost" })),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hosts() {
        assert_eq!(host_of("localhost:5173"), "localhost");
        assert_eq!(host_of("127.0.0.1"), "127.0.0.1");
        assert_eq!(host_of("[::1]:8787"), "[::1]");
    }

    #[test]
    fn allows_only_local_names_by_default() {
        let a = AllowedHosts::default();
        for ok in ["localhost", "LOCALHOST", "127.0.0.1", "[::1]", "weather.localhost"] {
            assert!(a.allows(ok), "{ok}");
        }
        for bad in ["evil.com", "localhost.evil.com", "127.0.0.1.nip.io", "192.168.1.5"] {
            assert!(!a.allows(bad), "{bad}");
        }
        let a = AllowedHosts { extra: vec!["devbox.lan".into()] };
        assert!(a.allows("devbox.lan"));
    }
}
