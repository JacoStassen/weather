use axum::{
    extract::{
        rejection::{JsonRejection, QueryRejection},
        Extension, Query, State,
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

use crate::{
    auth::{self, Access},
    custom_store,
    error::ApiError,
    service,
    state::AppState,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/health", get(health))
        .route("/providers", get(providers))
        .route("/weather/current", get(current))
        .route("/weather/forecast", get(forecast))
        .route("/alerts", get(alerts))
        .route("/marine", get(marine))
        .route("/blend", get(blend))
        .route("/geocode", get(geocode_search))
        .route("/geocode/reverse", get(geocode_reverse))
        .merge(
            Router::new()
                .route("/custom-providers", put(replace_custom_providers))
                .route("/custom-providers/test", post(test_custom_provider))
                .route_layer(axum::middleware::from_fn(auth::require_owner)),
        )
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

/// `ownerTokenRequired` tells the app whether to ask for the access token;
/// `owner` whether the token it sent (if any) was accepted.
async fn health(State(state): State<AppState>, Extension(access): Extension<Access>) -> Json<Value> {
    Json(json!({
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION"),
        "build": option_env!("WEATHER_BUILD"),
        "ownerTokenRequired": state.owner_token_required,
        "owner": access.owner,
    }))
}

async fn providers(
    State(state): State<AppState>,
    Extension(access): Extension<Access>,
) -> Json<Vec<ProviderInfo>> {
    let registry = state.registry.read().expect("registry lock poisoned");
    Json(registry.list().into_iter().filter(|p| access.may_use(&p.id)).collect())
}

async fn current(
    State(state): State<AppState>,
    Extension(access): Extension<Access>,
    q: Result<Query<PointQuery>, QueryRejection>,
) -> Result<Json<CurrentConditions>, ApiError> {
    let q = query(q)?;
    let point = GeoPoint::new(q.lat, q.lon);
    Ok(Json(service::current(&state, access, point, q.provider.as_deref()).await?))
}

async fn forecast(
    State(state): State<AppState>,
    Extension(access): Extension<Access>,
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
        service::forecast(&state, access, point, range, q.provider.as_deref()).await?,
    ))
}

async fn blend(
    State(state): State<AppState>,
    q: Result<Query<PointQuery>, QueryRejection>,
) -> Result<Json<weather_core::verify::BlendReport>, ApiError> {
    let q = query(q)?;
    let point = GeoPoint::new(q.lat, q.lon);
    if !point.is_valid() {
        return Err(ProviderError::InvalidLocation { lat: q.lat, lon: q.lon }.into());
    }
    Ok(Json(service::blend_report(&state, point)))
}

async fn alerts(
    State(state): State<AppState>,
    Extension(access): Extension<Access>,
    q: Result<Query<PointQuery>, QueryRejection>,
) -> Result<Json<AlertsResponse>, ApiError> {
    let q = query(q)?;
    Ok(Json(service::alerts(&state, access, GeoPoint::new(q.lat, q.lon)).await?))
}

async fn marine(
    State(state): State<AppState>,
    Extension(access): Extension<Access>,
    q: Result<Query<PointQuery>, QueryRejection>,
) -> Result<Json<MarineResponse>, ApiError> {
    let q = query(q)?;
    Ok(Json(service::marine(&state, access, GeoPoint::new(q.lat, q.lon)).await?))
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
        send(auth::OwnerToken::default(), Request::get(uri).body(Body::empty()).unwrap()).await
    }

    /// Like the real server: `identify` in front of the API.
    async fn send(token: auth::OwnerToken, req: Request<Body>) -> (StatusCode, Value) {
        let mut state = AppState::new(ProviderRegistry::new(), weather_core::http::default_client(), None);
        state.owner_token_required = token.is_set();
        let app = router()
            .with_state(state)
            .layer(axum::middleware::from_fn_with_state(token, auth::identify));
        let res = app.oneshot(req).await.unwrap();
        let status = res.status();
        let body = res.into_body().collect().await.unwrap().to_bytes();
        (status, serde_json::from_slice(&body).unwrap())
    }

    const TOKEN: &str = "0123456789abcdef0123456789abcdef";

    fn put_empty(bearer: Option<&str>) -> Request<Body> {
        let mut req = Request::put("/custom-providers").header("content-type", "application/json");
        if let Some(t) = bearer {
            req = req.header("authorization", format!("Bearer {t}"));
        }
        req.body(Body::from("[]")).unwrap()
    }

    #[tokio::test]
    async fn without_a_token_everyone_is_owner() {
        let (status, _) = send(auth::OwnerToken::default(), put_empty(None)).await;
        assert_eq!(status, 200);
        let (_, body) = get("/health").await;
        assert_eq!(body["ownerTokenRequired"], false);
        assert_eq!(body["owner"], true);
    }

    #[tokio::test]
    async fn with_a_token_writes_need_it() {
        let token = || auth::OwnerToken::new(TOKEN);
        let (status, body) = send(token(), put_empty(None)).await;
        assert_eq!(status, 401);
        assert_eq!(body["code"], "unauthorized");

        let (status, _) = send(token(), put_empty(Some("wrong-token"))).await;
        assert_eq!(status, 401);

        let (status, _) = send(token(), put_empty(Some(TOKEN))).await;
        assert_eq!(status, 200);
    }

    #[tokio::test]
    async fn with_a_token_visitors_still_read_weather() {
        let token = auth::OwnerToken::new(TOKEN);
        let (status, body) = send(token.clone(), Request::get("/health").body(Body::empty()).unwrap()).await;
        assert_eq!(status, 200);
        assert_eq!(body["ownerTokenRequired"], true);
        assert_eq!(body["owner"], false);

        // Reaches the handler (no providers registered, so 422), not a 401.
        let req = Request::get("/weather/forecast?lat=1&lon=1").body(Body::empty()).unwrap();
        let (status, _) = send(token, req).await;
        assert_eq!(status, 422);
    }

    #[tokio::test]
    async fn blend_report_starts_from_estimates() {
        let (status, body) = get("/blend?lat=-33.93&lon=18.42").await;
        assert_eq!(status, 200);
        assert!(body["station"].is_null());
        assert_eq!(body["sources"], serde_json::json!([]), "no blend sources in this router");

        let (status, body) = get("/blend?lat=123&lon=18.42").await;
        assert_eq!(status, 400, "{body}");
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
