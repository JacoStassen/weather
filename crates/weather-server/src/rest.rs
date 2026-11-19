use axum::{
    extract::{
        rejection::{JsonRejection, QueryRejection},
        Query, State,
    },
    http::StatusCode,
    routing::{get, post, put},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use weather_core::{
    geocode::{self, Place},
    providers::{custom::CustomProviderTestResult, CustomProvider, CustomProviderConfig},
    AlertsResponse, CurrentConditions, Forecast, ForecastRange, GeoPoint, MarineResponse,
    ProviderError, ProviderInfo,
};

use crate::{custom_store, error::ApiError, service, state::AppState};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/health", get(health))
        .route("/providers", get(providers))
        .route("/weather/current", get(current))
        .route("/weather/forecast", get(forecast))
        .route("/alerts", get(alerts))
        .route("/marine", get(marine))
        .route("/geocode", get(geocode_search))
        .route("/geocode/reverse", get(geocode_reverse))
        .route("/custom-providers", put(replace_custom_providers))
        .route("/custom-providers/test", post(test_custom_provider))
        // Keeps unknown /api paths from falling through to the web app shell.
        .fallback(not_found)
}

#[derive(Deserialize)]
struct PointQuery {
    lat: f64,
    lon: f64,
    provider: Option<String>,
}

#[derive(Deserialize)]
struct ForecastQuery {
    lat: f64,
    lon: f64,
    provider: Option<String>,
    hours: Option<u16>,
    days: Option<u8>,
}

#[derive(Deserialize)]
struct GeocodeQuery {
    q: String,
    limit: Option<u8>,
}

#[derive(Deserialize)]
struct TestRequest {
    config: CustomProviderConfig,
    lat: f64,
    lon: f64,
}

/// Turns axum's plain-text query rejection into our JSON error.
fn query<T>(q: Result<Query<T>, QueryRejection>) -> Result<T, ApiError> {
    q.map(|Query(t)| t).map_err(|e| ApiError::BadRequest(e.body_text()))
}

fn body<T>(b: Result<Json<T>, JsonRejection>) -> Result<T, ApiError> {
    b.map(|Json(t)| t).map_err(|e| ApiError::BadRequest(e.body_text()))
}

async fn health() -> Json<Value> {
    Json(json!({ "status": "ok", "version": env!("CARGO_PKG_VERSION") }))
}

async fn providers(State(state): State<AppState>) -> Json<Vec<ProviderInfo>> {
    let registry = state.registry.read().expect("registry lock poisoned");
    Json(registry.list())
}

async fn current(
    State(state): State<AppState>,
    q: Result<Query<PointQuery>, QueryRejection>,
) -> Result<Json<CurrentConditions>, ApiError> {
    let q = query(q)?;
    let point = GeoPoint::new(q.lat, q.lon);
    Ok(Json(service::current(&state, point, q.provider.as_deref()).await?))
}

async fn forecast(
    State(state): State<AppState>,
    q: Result<Query<ForecastQuery>, QueryRejection>,
) -> Result<Json<Forecast>, ApiError> {
    let q = query(q)?;
    let defaults = ForecastRange::default();
    let range = ForecastRange {
        hours: q.hours.unwrap_or(defaults.hours).clamp(1, 384),
        days: q.days.unwrap_or(defaults.days).clamp(1, 16),
    };
    let point = GeoPoint::new(q.lat, q.lon);
    Ok(Json(
        service::forecast(&state, point, range, q.provider.as_deref()).await?,
    ))
}

async fn alerts(
    State(state): State<AppState>,
    q: Result<Query<PointQuery>, QueryRejection>,
) -> Result<Json<AlertsResponse>, ApiError> {
    let q = query(q)?;
    Ok(Json(service::alerts(&state, GeoPoint::new(q.lat, q.lon)).await?))
}

async fn marine(
    State(state): State<AppState>,
    q: Result<Query<PointQuery>, QueryRejection>,
) -> Result<Json<MarineResponse>, ApiError> {
    let q = query(q)?;
    Ok(Json(service::marine(&state, GeoPoint::new(q.lat, q.lon)).await?))
}

async fn geocode_search(
    State(state): State<AppState>,
    q: Result<Query<GeocodeQuery>, QueryRejection>,
) -> Result<Json<Vec<Place>>, ApiError> {
    let q = query(q)?;
    Ok(Json(geocode::search(&state.http, &q.q, q.limit.unwrap_or(8)).await?))
}

/// Names the place at a point (for "use my location"). `null` over open sea.
async fn geocode_reverse(
    State(state): State<AppState>,
    q: Result<Query<PointQuery>, QueryRejection>,
) -> Result<Json<Option<Place>>, ApiError> {
    let q = query(q)?;
    if !GeoPoint::new(q.lat, q.lon).is_valid() {
        return Err(ProviderError::InvalidLocation { lat: q.lat, lon: q.lon }.into());
    }
    let key = ((q.lat * 100.0).round() as i32, (q.lon * 100.0).round() as i32);
    if let Some(hit) = state.place_names.get(&key) {
        return Ok(Json(hit));
    }
    let place = geocode::reverse(&state.http, q.lat, q.lon).await?;
    state.place_names.insert(key, place.clone());
    Ok(Json(place))
}

/// The web app sends its full list of custom providers whenever it changes.
async fn replace_custom_providers(
    State(state): State<AppState>,
    b: Result<Json<Vec<CustomProviderConfig>>, JsonRejection>,
) -> Result<Json<Vec<ProviderInfo>>, ApiError> {
    let configs = body(b)?;
    let list = custom_store::replace_all(&state, configs)
        .await
        .map_err(ApiError::BadRequest)?;
    Ok(Json(list))
}

/// Tries a config without saving it. Upstream failures are reported in the
/// result (with the raw response when there is one), not as HTTP errors.
async fn test_custom_provider(
    State(state): State<AppState>,
    b: Result<Json<TestRequest>, JsonRejection>,
) -> Result<Json<CustomProviderTestResult>, ApiError> {
    let req = body(b)?;
    let point = GeoPoint::new(req.lat, req.lon);
    if !point.is_valid() {
        return Err(ProviderError::InvalidLocation { lat: req.lat, lon: req.lon }.into());
    }
    let provider = CustomProvider::new(req.config, state.http.clone()).map_err(ApiError::BadRequest)?;
    Ok(Json(provider.test(point).await))
}

async fn not_found() -> (StatusCode, Json<Value>) {
    (
        StatusCode::NOT_FOUND,
        Json(json!({ "code": "not_found", "message": "unknown API route" })),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request};
    use http_body_util::BodyExt;
    use tower::ServiceExt;
    use weather_core::ProviderRegistry;

    /// Router with no providers registered: exercises routing, validation and
    /// error mapping without touching the network.
    async fn get(uri: &str) -> (StatusCode, Value) {
        let state = AppState::new(ProviderRegistry::new(), weather_core::http::default_client(), None);
        let app = router().with_state(state);
        let res = app
            .oneshot(Request::get(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = res.status();
        let body = res.into_body().collect().await.unwrap().to_bytes();
        (status, serde_json::from_slice(&body).unwrap())
    }

    #[tokio::test]
    async fn health_returns_ok() {
        let (status, body) = get("/health").await;
        assert_eq!(status, 200);
        assert_eq!(body["status"], "ok");
    }

    #[tokio::test]
    async fn missing_or_malformed_query_is_json_400() {
        for uri in ["/weather/current", "/weather/current?lat=abc&lon=1"] {
            let (status, body) = get(uri).await;
            assert_eq!(status, 400, "{uri}");
            assert_eq!(body["code"], "bad_request");
        }
    }

    #[tokio::test]
    async fn out_of_range_coordinates_are_400() {
        let (status, body) = get("/weather/current?lat=91&lon=0").await;
        assert_eq!(status, 400);
        assert_eq!(body["code"], "invalid_location");
    }

    #[tokio::test]
    async fn unknown_provider_is_400() {
        let (status, body) = get("/weather/current?lat=1&lon=1&provider=nope").await;
        assert_eq!(status, 400);
        assert_eq!(body["code"], "unknown_provider");
    }

    #[tokio::test]
    async fn no_provider_is_422() {
        let (status, body) = get("/weather/forecast?lat=1&lon=1").await;
        assert_eq!(status, 422);
        assert_eq!(body["code"], "no_provider");
    }

    #[tokio::test]
    async fn uncovered_alerts_and_marine_succeed_empty() {
        let (status, body) = get("/alerts?lat=52.37&lon=4.9").await;
        assert_eq!(status, 200);
        assert_eq!(body, json!({ "coverage": false, "providerId": null, "alerts": [] }));

        let (status, body) = get("/marine?lat=52.37&lon=4.9").await;
        assert_eq!(status, 200);
        assert_eq!(body, json!({ "coastal": false, "report": null }));
    }

    #[tokio::test]
    async fn short_geocode_query_returns_empty_without_network() {
        let (status, body) = get("/geocode?q=a").await;
        assert_eq!(status, 200);
        assert_eq!(body, json!([]));
    }

    #[tokio::test]
    async fn unknown_route_is_json_404() {
        let (status, body) = get("/nope").await;
        assert_eq!(status, 404);
        assert_eq!(body["code"], "not_found");
    }
}
