use crate::types::Capability;

#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("{provider}: network error: {message}")]
    Network { provider: String, message: String },

    #[error("{provider}: authentication failed (check the API key)")]
    Auth { provider: String },

    #[error("{provider}: rate limited")]
    RateLimited {
        provider: String,
        retry_after_secs: Option<u64>,
    },

    #[error("{provider} does not cover {lat:.3},{lon:.3}")]
    UnsupportedLocation { provider: String, lat: f64, lon: f64 },

    #[error("{provider} does not provide {capability:?} data")]
    UnsupportedCapability {
        provider: String,
        capability: Capability,
    },

    #[error("{provider}: unexpected response: {message}")]
    BadResponse { provider: String, message: String },

    #[error("no provider can supply {capability:?} data for this location")]
    NoProvider { capability: Capability },

    #[error("unknown provider '{0}'")]
    UnknownProvider(String),

    #[error("invalid coordinates {lat},{lon}")]
    InvalidLocation { lat: f64, lon: f64 },
}

impl ProviderError {
    /// Stable machine-readable error code for API and MCP responses.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Network { .. } => "network",
            Self::Auth { .. } => "auth",
            Self::RateLimited { .. } => "rate_limited",
            Self::UnsupportedLocation { .. } => "unsupported_location",
            Self::UnsupportedCapability { .. } => "unsupported_capability",
            Self::BadResponse { .. } => "bad_response",
            Self::NoProvider { .. } => "no_provider",
            Self::UnknownProvider(_) => "unknown_provider",
            Self::InvalidLocation { .. } => "invalid_location",
        }
    }
}
