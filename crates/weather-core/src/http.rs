//! Shared HTTP plumbing for providers: one client, one error mapping.

use std::time::Duration;

use reqwest::{header::RETRY_AFTER, Client, RequestBuilder, StatusCode};
use serde::{de::DeserializeOwned, Deserialize};

use crate::error::ProviderError;

/// Sent with every upstream request. NWS rejects requests without one.
pub const USER_AGENT: &str = concat!(
    "weather-app/",
    env!("CARGO_PKG_VERSION"),
    " (local weather dashboard)"
);

pub fn default_client() -> Client {
    Client::builder()
        .user_agent(USER_AGENT)
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(15))
        .build()
        .expect("static reqwest client configuration is valid")
}

/// Sends `req` and decodes a JSON body, mapping HTTP failures to [`ProviderError`].
pub(crate) async fn get_json<T: DeserializeOwned>(
    provider: &str,
    req: RequestBuilder,
) -> Result<T, ProviderError> {
    let body = get_bytes(provider, req).await?;
    serde_json::from_slice(&body).map_err(|e| ProviderError::BadResponse {
        provider: provider.to_owned(),
        message: format!("could not parse response: {e}"),
    })
}

/// Sends `req` and returns the body, mapping HTTP failures to [`ProviderError`].
pub(crate) async fn get_bytes(provider: &str, req: RequestBuilder) -> Result<bytes::Bytes, ProviderError> {
    let network = |e: reqwest::Error| ProviderError::Network {
        provider: provider.to_owned(),
        message: describe(e),
    };

    let res = req.send().await.map_err(network)?;
    let status = res.status();
    let retry_after_secs = res
        .headers()
        .get(RETRY_AFTER)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse().ok());
    let body = res.bytes().await.map_err(network)?;

    if !status.is_success() {
        let provider = provider.to_owned();
        return Err(match status {
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => ProviderError::Auth { provider },
            StatusCode::TOO_MANY_REQUESTS => ProviderError::RateLimited {
                provider,
                retry_after_secs,
            },
            s if s.is_server_error() => ProviderError::Network {
                provider,
                message: format!("upstream returned {s}"),
            },
            s => ProviderError::BadResponse {
                provider,
                message: format!("{s}: {}", error_detail(&body)),
            },
        });
    }

    Ok(body)
}

/// Error text without the request URL, which may carry an API key.
fn describe(e: reqwest::Error) -> String {
    if e.is_timeout() {
        "request timed out".into()
    } else if e.is_connect() {
        "could not connect".into()
    } else {
        e.without_url().to_string()
    }
}

/// Pulls a human-readable reason out of common JSON error bodies
/// (Open-Meteo: `reason`, NWS problem+json: `detail`).
fn error_detail(body: &[u8]) -> String {
    #[derive(Deserialize)]
    struct Body {
        reason: Option<String>,
        detail: Option<String>,
        message: Option<String>,
    }
    serde_json::from_slice::<Body>(body)
        .ok()
        .and_then(|b| b.reason.or(b.detail).or(b.message))
        .unwrap_or_else(|| String::from_utf8_lossy(&body[..body.len().min(200)]).into_owned())
}

/// Re-exported so callers can hold a client without depending on reqwest.
pub use reqwest::Client as HttpClient;
