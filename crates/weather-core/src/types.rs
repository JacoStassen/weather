//! Normalized domain models. All units are metric: °C, m/s, hPa, mm, metres.
//! Location-local timestamps carry their UTC offset (RFC 3339 on the wire).

use chrono::{DateTime, FixedOffset, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use schemars::JsonSchema;
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
#[ts(export)]
pub struct GeoPoint {
    pub lat: f64,
    pub lon: f64,
}

impl GeoPoint {
    pub fn new(lat: f64, lon: f64) -> Self {
        Self { lat, lon }
    }

    pub fn is_valid(&self) -> bool {
        self.lat.is_finite()
            && self.lon.is_finite()
            && (-90.0..=90.0).contains(&self.lat)
            && (-180.0..=180.0).contains(&self.lon)
    }
}

/// A kind of data a provider can supply.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Capability {
    Current,
    Hourly,
    Daily,
    Alerts,
    Marine,
}

/// Public description of a registered provider.
#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ProviderInfo {
    pub id: String,
    pub name: String,
    pub capabilities: Vec<Capability>,
}

/// Which provider produced a piece of data, and when.
#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SourceMeta {
    pub provider_id: String,
    pub fetched_at: DateTime<Utc>,
}

/// Normalized sky/precipitation condition, independent of any provider's codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Condition {
    Clear,
    PartlyCloudy,
    Cloudy,
    Fog,
    Drizzle,
    Rain,
    FreezingRain,
    Snow,
    Sleet,
    Thunderstorm,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CurrentConditions {
    pub observed_at: DateTime<FixedOffset>,
    pub temperature_c: f64,
    pub feels_like_c: Option<f64>,
    pub humidity_pct: Option<f64>,
    pub wind_speed_ms: Option<f64>,
    pub wind_gust_ms: Option<f64>,
    pub wind_dir_deg: Option<f64>,
    pub pressure_hpa: Option<f64>,
    pub precip_mm: Option<f64>,
    pub cloud_cover_pct: Option<f64>,
    pub uv_index: Option<f64>,
    pub condition: Condition,
    pub is_day: bool,
    pub source: SourceMeta,
}

/// How far ahead a forecast should reach.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ForecastRange {
    pub hours: u16,
    pub days: u8,
}

impl Default for ForecastRange {
    fn default() -> Self {
        Self { hours: 48, days: 7 }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct HourlyPoint {
    pub time: DateTime<FixedOffset>,
    pub temperature_c: f64,
    pub precip_prob_pct: Option<f64>,
    pub precip_mm: Option<f64>,
    pub wind_speed_ms: Option<f64>,
    pub condition: Condition,
    pub is_day: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DailyPoint {
    pub date: NaiveDate,
    pub min_c: f64,
    pub max_c: f64,
    pub precip_mm: Option<f64>,
    pub precip_prob_pct: Option<f64>,
    pub sunrise: Option<DateTime<FixedOffset>>,
    pub sunset: Option<DateTime<FixedOffset>>,
    pub condition: Condition,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Forecast {
    /// IANA timezone of the location, e.g. "Europe/Amsterdam".
    pub timezone: String,
    pub hourly: Vec<HourlyPoint>,
    pub daily: Vec<DailyPoint>,
    pub source: SourceMeta,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Severity {
    Unknown,
    Minor,
    Moderate,
    Severe,
    Extreme,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct WeatherAlert {
    pub id: String,
    pub event: String,
    pub severity: Severity,
    pub headline: String,
    pub description: String,
    pub instruction: Option<String>,
    pub effective: DateTime<FixedOffset>,
    pub expires: Option<DateTime<FixedOffset>>,
    pub area_desc: String,
    /// The issuer's own warning level, e.g. "Warning Level 4" (South African
    /// Weather Service impact-based warnings). `None` when it has none.
    #[serde(default)]
    pub level: Option<String>,
    pub source: SourceMeta,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct WavePoint {
    pub time: DateTime<FixedOffset>,
    pub wave_height_m: Option<f64>,
    pub wave_period_s: Option<f64>,
    pub wave_dir_deg: Option<f64>,
    pub swell_height_m: Option<f64>,
    pub sea_temp_c: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TideSample {
    pub time: DateTime<FixedOffset>,
    pub height_m: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum TideKind {
    High,
    Low,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TideEvent {
    pub time: DateTime<FixedOffset>,
    pub kind: TideKind,
    pub height_m: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TideData {
    /// Vertical reference for heights, e.g. "MSL".
    pub datum: String,
    /// True when tides come from a model rather than station predictions.
    /// Modeled tides are not suitable for navigation.
    pub modeled: bool,
    pub series: Vec<TideSample>,
    pub events: Vec<TideEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MarineReport {
    pub waves: Vec<WavePoint>,
    /// `None` where the sea-level model has no data, e.g. on large lakes,
    /// which get waves but no tides.
    pub tides: Option<TideData>,
    pub source: SourceMeta,
}

/// Response of the alerts endpoint and the `get_severe_alerts` MCP tool.
#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AlertsResponse {
    /// False when no alert provider covers the location. An empty `alerts`
    /// list then means "unknown", not "all clear".
    pub coverage: bool,
    pub provider_id: Option<String>,
    pub alerts: Vec<WeatherAlert>,
}

/// Response of the marine endpoint and the `get_marine_tides` MCP tool.
#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MarineResponse {
    /// True when the location has marine data (it is on or near open water).
    pub coastal: bool,
    pub report: Option<MarineReport>,
}
