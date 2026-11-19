use axum::{
    http::{header::RETRY_AFTER, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use weather_core::ProviderError;

/// Error returned by REST handlers as `{ "code": ..., "message": ... }`.
#[derive(Debug)]
pub enum ApiError {
    BadRequest(String),
    Provider(ProviderError),
}

impl From<ProviderError> for ApiError {
    fn from(e: ProviderError) -> Self {
        Self::Provider(e)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, code, message, retry_after) = match self {
            Self::BadRequest(message) => (StatusCode::BAD_REQUEST, "bad_request", message, None),
            Self::Provider(e) => {
                let status = match &e {
                    ProviderError::InvalidLocation { .. } | ProviderError::UnknownProvider(_) => {
                        StatusCode::BAD_REQUEST
                    }
                    ProviderError::NoProvider { .. }
                    | ProviderError::UnsupportedCapability { .. }
                    | ProviderError::UnsupportedLocation { .. } => StatusCode::UNPROCESSABLE_ENTITY,
                    ProviderError::RateLimited { .. } => StatusCode::TOO_MANY_REQUESTS,
                    ProviderError::Auth { .. }
                    | ProviderError::Network { .. }
                    | ProviderError::BadResponse { .. } => StatusCode::BAD_GATEWAY,
                };
                let retry = match &e {
                    ProviderError::RateLimited { retry_after_secs, .. } => *retry_after_secs,
                    _ => None,
                };
                (status, e.code(), e.to_string(), retry)
            }
        };

        if status.is_server_error() {
            tracing::warn!(code, %message, "upstream provider error");
        }
        let mut res = (status, Json(json!({ "code": code, "message": message }))).into_response();
        if let Some(secs) = retry_after {
            res.headers_mut().insert(RETRY_AFTER, HeaderValue::from(secs));
        }
        res
    }
}
