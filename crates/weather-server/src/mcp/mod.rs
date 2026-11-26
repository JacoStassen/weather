//! MCP server: exposes the weather service as tools for AI agents.
//!
//! Served over Streamable HTTP at `/mcp` (current spec; streams over SSE) and
//! over the legacy HTTP+SSE transport at `/sse` for older clients. Both use
//! the same [`WeatherMcp`] handler and the same service layer as the REST API.

pub mod legacy_sse;

use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{CallToolResult, ContentBlock, Implementation, ServerCapabilities, ServerConfig},
    tool, tool_handler, tool_router, Json, ServerHandler,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use weather_core::{
    geocode::{self, Place},
    AlertsResponse, CurrentConditions, DailyPoint, ForecastRange, GeoPoint, HourlyPoint,
    ProviderError, ProviderInfo, TideEvent, WavePoint,
};

use crate::{auth::Access, service, state::AppState};

const INSTRUCTIONS: &str = "Weather data for any location, by latitude/longitude. \
If you only have a place name, call search_places first to get coordinates. \
Temperatures are °C, wind m/s, pressure hPa, precipitation mm, heights m; \
timestamps are RFC 3339 in the location's local offset. \
Severe weather alerts cover the US (National Weather Service) and South Africa \
(South African Weather Service) only: check `coverage` before telling the user there \
are no alerts. Tides are modeled, not station predictions, and must not be \
used for navigation.";

#[derive(Clone)]
pub struct WeatherMcp {
    state: AppState,
    tool_router: ToolRouter<Self>,
}

// ---- Tool arguments --------------------------------------------------------

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CurrentWeatherArgs {
    /// Latitude in decimal degrees (-90 to 90).
    pub lat: f64,
    /// Longitude in decimal degrees (-180 to 180).
    pub lon: f64,
    /// Provider id from list_providers, e.g. "open-meteo" or "custom:my-api".
    /// Omit to use the default. If the provider can't serve the request, the
    /// default is used; `source.providerId` in the result says which answered.
    #[serde(default)]
    pub provider: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PointArgs {
    /// Latitude in decimal degrees (-90 to 90).
    pub lat: f64,
    /// Longitude in decimal degrees (-180 to 180).
    pub lon: f64,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ForecastArgs {
    /// Latitude in decimal degrees (-90 to 90).
    pub lat: f64,
    /// Longitude in decimal degrees (-180 to 180).
    pub lon: f64,
    /// Hours of hourly forecast to return, 0-48. Default 24.
    #[serde(default)]
    pub hours: Option<u16>,
    /// Days of daily forecast to return, 1-16. Default 7.
    #[serde(default)]
    pub days: Option<u8>,
    /// Provider id from list_providers. Omit to use the default.
    #[serde(default)]
    pub provider: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SearchArgs {
    /// Place name, e.g. "Lisbon" or "Portland, Maine". English names work best.
    pub query: String,
    /// Maximum results, 1-10. Default 5.
    #[serde(default)]
    pub limit: Option<u8>,
}

// ---- Tool outputs ----------------------------------------------------------

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ForecastOutput {
    pub timezone: String,
    pub provider_id: String,
    pub hourly: Vec<HourlyPoint>,
    pub daily: Vec<DailyPoint>,
}

/// Compact marine summary: the full hourly series is too long to be useful
/// in a model's context.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MarineTidesOutput {
    /// False for inland locations; everything else is then empty.
    pub coastal: bool,
    /// False where there is open water but no tide model (e.g. large lakes).
    pub has_tides: bool,
    /// Upcoming high and low tides (heights relative to `tideDatum`).
    pub tide_events: Vec<TideEvent>,
    pub tide_datum: Option<String>,
    /// Waves for the current hour.
    pub waves_now: Option<WavePoint>,
    /// Waves every 3 hours for the next 24 hours.
    pub wave_outlook: Vec<WavePoint>,
    pub note: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct PlacesOutput {
    pub places: Vec<Place>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ProvidersOutput {
    pub providers: Vec<ProviderInfo>,
}

/// Tool methods spell out `Result<Json<T>, CallToolResult>`: the macro only
/// derives an output schema when it sees `Json<T>` in the signature.
///
/// Tool failures go back as `isError` results (not protocol errors), so the
/// model sees the reason and can correct its call.
fn tool_error(e: ProviderError) -> CallToolResult {
    CallToolResult::error(vec![ContentBlock::text(format!("{e} [{}]", e.code()))])
}

#[tool_router]
impl WeatherMcp {
    pub fn new(state: AppState) -> Self {
        Self {
            state,
            tool_router: Self::tool_router(),
        }
    }

    #[tool(
        description = "Current weather conditions at a location: temperature, feels-like, humidity, wind, pressure, precipitation, cloud cover, UV index and a condition (clear, rain, snow, ...).",
        annotations(title = "Current weather", read_only_hint = true, open_world_hint = true)
    )]
    async fn get_current_weather(
        &self,
        Parameters(a): Parameters<CurrentWeatherArgs>,
    ) -> Result<Json<CurrentConditions>, CallToolResult> {
        let point = GeoPoint::new(a.lat, a.lon);
        service::current(&self.state, Access::OWNER, point, a.provider.as_deref())
            .await
            .map(Json)
            .map_err(tool_error)
    }

    #[tool(
        description = "Active severe weather watches, warnings and advisories at a location, most severe first, including ones issued ahead for the coming days (see `effective`). Covers the US (National Weather Service) and South Africa (South African Weather Service; `level` is its impact-based warning level). When `coverage` is false, no alert data exists for the location, which does not mean it is safe.",
        annotations(title = "Severe weather alerts", read_only_hint = true, open_world_hint = true)
    )]
    async fn get_severe_alerts(&self, Parameters(a): Parameters<PointArgs>) -> Result<Json<AlertsResponse>, CallToolResult> {
        service::alerts(&self.state, Access::OWNER, GeoPoint::new(a.lat, a.lon))
            .await
            .map(Json)
            .map_err(tool_error)
    }

    #[tool(
        description = "Tide times and wave conditions for a coastal location: upcoming high/low tides, current waves and a 24-hour wave outlook. Returns coastal=false for inland locations. Tides are modeled and not for navigation.",
        annotations(title = "Marine & tides", read_only_hint = true, open_world_hint = true)
    )]
    async fn get_marine_tides(&self, Parameters(a): Parameters<PointArgs>) -> Result<Json<MarineTidesOutput>, CallToolResult> {
        let res = service::marine(&self.state, Access::OWNER, GeoPoint::new(a.lat, a.lon))
            .await
            .map_err(tool_error)?;
        Ok(Json(summarize_marine(res.report, chrono::Utc::now())))
    }

    #[tool(
        description = "Hourly and daily weather forecast for a location.",
        annotations(title = "Forecast", read_only_hint = true, open_world_hint = true)
    )]
    async fn get_forecast(&self, Parameters(a): Parameters<ForecastArgs>) -> Result<Json<ForecastOutput>, CallToolResult> {
        let hours = a.hours.unwrap_or(24).min(48);
        let range = ForecastRange {
            // Fetch the dashboard's default range so the cache is shared.
            hours: ForecastRange::default().hours.max(hours),
            days: a.days.unwrap_or(7).clamp(1, 16),
        };
        let f = service::forecast(&self.state, Access::OWNER, GeoPoint::new(a.lat, a.lon), range, a.provider.as_deref())
            .await
            .map_err(tool_error)?;
        let now = chrono::Utc::now();
        Ok(Json(ForecastOutput {
            timezone: f.timezone,
            provider_id: f.source.provider_id,
            hourly: f
                .hourly
                .into_iter()
                .filter(|h| h.time > now - chrono::Duration::hours(1))
                .take(hours as usize)
                .collect(),
            daily: f.daily.into_iter().take(range.days as usize).collect(),
        }))
    }

    #[tool(
        description = "Find places by name and get their coordinates, for use with the other tools.",
        annotations(title = "Search places", read_only_hint = true, open_world_hint = true)
    )]
    async fn search_places(&self, Parameters(a): Parameters<SearchArgs>) -> Result<Json<PlacesOutput>, CallToolResult> {
        let limit = a.limit.unwrap_or(5).clamp(1, 10);
        geocode::search(&self.state.http, &a.query, limit)
            .await
            .map(|places| Json(PlacesOutput { places }))
            .map_err(tool_error)
    }

    #[tool(
        description = "List the weather data providers available for the `provider` argument, with what each can supply.",
        annotations(title = "List providers", read_only_hint = true, open_world_hint = false)
    )]
    async fn list_providers(&self) -> Json<ProvidersOutput> {
        let providers = self.state.registry.read().expect("registry lock poisoned").list();
        Json(ProvidersOutput { providers })
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for WeatherMcp {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(
                Implementation::new("weather", env!("CARGO_PKG_VERSION")).with_title("Weather"),
            )
            .with_instructions(INSTRUCTIONS)
    }
}

fn summarize_marine(
    report: Option<weather_core::MarineReport>,
    now: chrono::DateTime<chrono::Utc>,
) -> MarineTidesOutput {
    let Some(r) = report else {
        return MarineTidesOutput {
            coastal: false,
            has_tides: false,
            tide_events: vec![],
            tide_datum: None,
            waves_now: None,
            wave_outlook: vec![],
            note: Some("No marine data: the location is not on or near open water.".into()),
        };
    };
    let hour_ago = now - chrono::Duration::hours(1);
    let upcoming: Vec<WavePoint> = r.waves.into_iter().filter(|w| w.time > hour_ago).collect();
    let (tide_events, tide_datum) = match &r.tides {
        Some(t) => (
            t.events.iter().filter(|e| e.time > hour_ago).cloned().collect(),
            Some(t.datum.clone()),
        ),
        None => (vec![], None),
    };
    MarineTidesOutput {
        coastal: true,
        has_tides: r.tides.is_some(),
        tide_events,
        tide_datum,
        waves_now: upcoming.first().cloned(),
        wave_outlook: upcoming.iter().take(25).step_by(3).skip(1).cloned().collect(),
        note: Some(match &r.tides {
            Some(t) if t.modeled => {
                "Tide times are modeled from Open-Meteo sea-level data, not station predictions. Not for navigation.".into()
            }
            Some(_) => "Tide times from station predictions.".into(),
            None => "Waves only: no tide model for this water body.".into(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use async_trait::async_trait;
    use chrono::{Duration, FixedOffset, TimeZone, Utc};
    use rmcp::{model::CallToolRequestParams, service::RunningService, RoleClient, ServiceExt};
    use serde_json::{json, Value};
    use weather_core::{
        Capability, Condition, CurrentConditions, MarineReport, ProviderRegistry, SourceMeta, TideData, TideKind,
        WeatherProvider,
    };

    use super::*;

    /// Offline provider: current conditions everywhere, marine data only east of 0°.
    struct Fake;

    fn src() -> SourceMeta {
        SourceMeta { provider_id: "fake".into(), fetched_at: Utc::now() }
    }

    #[async_trait]
    impl WeatherProvider for Fake {
        fn id(&self) -> &str {
            "fake"
        }
        fn name(&self) -> &str {
            "Fake"
        }
        fn capabilities(&self) -> &[Capability] {
            &[Capability::Current, Capability::Marine]
        }
        async fn current(&self, _p: GeoPoint) -> Result<CurrentConditions, ProviderError> {
            Ok(CurrentConditions {
                observed_at: Utc::now().fixed_offset(),
                temperature_c: 12.5,
                feels_like_c: None,
                humidity_pct: Some(80.0),
                wind_speed_ms: None,
                wind_gust_ms: None,
                wind_dir_deg: None,
                pressure_hpa: None,
                precip_mm: None,
                cloud_cover_pct: None,
                uv_index: None,
                condition: Condition::Rain,
                is_day: true,
                source: src(),
            })
        }
        async fn marine(&self, p: GeoPoint) -> Result<Option<MarineReport>, ProviderError> {
            if p.lon < 0.0 {
                return Ok(None);
            }
            Ok(Some(marine_report(Utc::now())))
        }
    }

    /// 48 hourly wave points from 6 h ago, and a tide turn every ~6 h.
    fn marine_report(now: chrono::DateTime<Utc>) -> MarineReport {
        let off = FixedOffset::east_opt(3600).unwrap();
        let start = off.timestamp_opt(now.timestamp() - 6 * 3600, 0).unwrap();
        MarineReport {
            waves: (0..48)
                .map(|h| WavePoint {
                    time: start + Duration::hours(h),
                    wave_height_m: Some(1.0),
                    wave_period_s: None,
                    wave_dir_deg: None,
                    swell_height_m: None,
                    sea_temp_c: None,
                })
                .collect(),
            tides: Some(TideData {
                datum: "MSL".into(),
                modeled: true,
                series: vec![],
                events: (0..8)
                    .map(|i| TideEvent {
                        time: start + Duration::minutes(i * 372),
                        kind: if i % 2 == 0 { TideKind::High } else { TideKind::Low },
                        height_m: if i % 2 == 0 { 0.8 } else { -0.6 },
                    })
                    .collect(),
            }),
            source: src(),
        }
    }

    async fn connect(registry: ProviderRegistry) -> RunningService<RoleClient, ()> {
        let state = AppState::new(registry, weather_core::http::default_client(), None);
        let (server_io, client_io) = tokio::io::duplex(1 << 16);
        tokio::spawn(async move {
            let server = WeatherMcp::new(state).serve(server_io).await.unwrap();
            let _ = server.waiting().await;
        });
        ().serve(client_io).await.unwrap()
    }

    fn fake_registry() -> ProviderRegistry {
        let mut r = ProviderRegistry::new().with_fallback("fake");
        r.register(Arc::new(Fake));
        r
    }

    async fn call(client: &RunningService<RoleClient, ()>, tool: &'static str, args: Value) -> CallToolResult {
        let Value::Object(args) = args else { panic!("args must be an object") };
        client
            .call_tool(CallToolRequestParams::new(tool).with_arguments(args))
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn lists_tools_with_schemas_and_read_only_hints() {
        let client = connect(ProviderRegistry::new()).await;
        let tools = client.list_all_tools().await.unwrap();
        let mut names: Vec<_> = tools.iter().map(|t| t.name.to_string()).collect();
        names.sort();
        assert_eq!(
            names,
            [
                "get_current_weather",
                "get_forecast",
                "get_marine_tides",
                "get_severe_alerts",
                "list_providers",
                "search_places"
            ]
        );
        for t in &tools {
            assert!(t.output_schema.is_some(), "{} has no output schema", t.name);
            assert_eq!(t.annotations.as_ref().and_then(|a| a.read_only_hint), Some(true), "{}", t.name);
        }
        let current = tools.iter().find(|t| t.name == "get_current_weather").unwrap();
        let required = current.input_schema.get("required").unwrap();
        assert_eq!(required, &json!(["lat", "lon"]));
    }

    #[tokio::test]
    async fn current_weather_returns_structured_content() {
        let client = connect(fake_registry()).await;
        let r = call(&client, "get_current_weather", json!({ "lat": 52.1, "lon": 4.3 })).await;
        assert_ne!(r.is_error, Some(true));
        let s = r.structured_content.unwrap();
        assert_eq!(s["temperatureC"], 12.5);
        assert_eq!(s["condition"], "rain");
        assert_eq!(s["source"]["providerId"], "fake");
    }

    #[tokio::test]
    async fn bad_input_is_a_tool_error_the_model_can_read() {
        let client = connect(fake_registry()).await;
        let r = call(&client, "get_current_weather", json!({ "lat": 123, "lon": 4 })).await;
        assert_eq!(r.is_error, Some(true));
        let text = r.content[0].as_text().unwrap().text.clone();
        assert!(text.contains("invalid_location"), "{text}");

        let r = call(&client, "get_current_weather", json!({ "lat": 1, "lon": 1, "provider": "nope" })).await;
        assert_eq!(r.is_error, Some(true));
    }

    #[tokio::test]
    async fn alerts_outside_coverage_say_so() {
        let client = connect(fake_registry()).await;
        let r = call(&client, "get_severe_alerts", json!({ "lat": 52.1, "lon": 4.3 })).await;
        assert_eq!(r.structured_content.unwrap(), json!({ "coverage": false, "providerId": null, "alerts": [] }));
    }

    #[tokio::test]
    async fn marine_tides_coastal_and_inland() {
        let client = connect(fake_registry()).await;
        let r = call(&client, "get_marine_tides", json!({ "lat": 52.1, "lon": 4.3 })).await;
        let s = r.structured_content.unwrap();
        assert_eq!(s["coastal"], true);
        assert_eq!(s["hasTides"], true);
        assert!(s["note"].as_str().unwrap().contains("Not for navigation"));

        let r = call(&client, "get_marine_tides", json!({ "lat": 40.0, "lon": -100.0 })).await;
        let s = r.structured_content.unwrap();
        assert_eq!(s["coastal"], false);
        assert_eq!(s["tideEvents"], json!([]));
    }

    #[test]
    fn marine_summary_keeps_upcoming_and_samples_every_three_hours() {
        let now = Utc::now();
        let out = summarize_marine(Some(marine_report(now)), now);
        let hour_ago = now - Duration::hours(1);
        assert!(out.tide_events.iter().all(|e| e.time > hour_ago));
        assert!(!out.tide_events.is_empty());
        let first = out.waves_now.unwrap().time;
        assert!(first > hour_ago && first <= now + Duration::hours(1));
        assert_eq!(out.wave_outlook.len(), 8);
        for (i, w) in out.wave_outlook.iter().enumerate() {
            assert_eq!(w.time - first, Duration::hours(3 * (i as i64 + 1)));
        }
    }
}
