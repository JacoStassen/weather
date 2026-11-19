//! Open-Meteo (<https://open-meteo.com>): global forecasts and marine data, no API key.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, FixedOffset, Utc};
use serde::Deserialize;

use crate::cache::TtlCache;
use crate::error::ProviderError;
use crate::http::get_json;
use crate::provider::WeatherProvider;
use crate::tides;
use crate::types::*;

pub const ID: &str = "open-meteo";
pub const FORECAST_URL: &str = "https://api.open-meteo.com/v1/forecast";
pub const MARINE_URL: &str = "https://marine-api.open-meteo.com/v1/marine";

const CAPABILITIES: &[Capability] = &[
    Capability::Current,
    Capability::Hourly,
    Capability::Daily,
    Capability::Marine,
];

const CURRENT_VARS: &str = "temperature_2m,apparent_temperature,relative_humidity_2m,is_day,\
precipitation,weather_code,cloud_cover,pressure_msl,wind_speed_10m,wind_direction_10m,\
wind_gusts_10m,uv_index";
const HOURLY_VARS: &str =
    "temperature_2m,precipitation_probability,precipitation,weather_code,wind_speed_10m,is_day";
const DAILY_VARS: &str = "weather_code,temperature_2m_max,temperature_2m_min,precipitation_sum,\
precipitation_probability_max,sunrise,sunset";
const MARINE_VARS: &str = "wave_height,wave_direction,wave_period,swell_wave_height,\
sea_surface_temperature,sea_level_height_msl";
const MARINE_DAYS: u8 = 3;

const FORECAST_TTL: Duration = Duration::from_secs(10 * 60);
const MARINE_TTL: Duration = Duration::from_secs(30 * 60);

/// Coordinates rounded to 0.01° (~1 km), used both as cache key and in the
/// request, so nearby lookups share one upstream call.
type CellKey = (i32, i32);

fn cell(p: GeoPoint) -> CellKey {
    ((p.lat * 100.0).round() as i32, (p.lon * 100.0).round() as i32)
}

fn cell_coords(k: CellKey) -> (String, String) {
    (
        format!("{:.2}", k.0 as f64 / 100.0),
        format!("{:.2}", k.1 as f64 / 100.0),
    )
}

struct Fetched<T> {
    at: DateTime<Utc>,
    data: T,
}

pub struct OpenMeteoProvider {
    client: reqwest::Client,
    forecast_url: String,
    marine_url: String,
    forecasts: TtlCache<(CellKey, ForecastRange), Arc<Fetched<OmForecast>>>,
    marine: TtlCache<CellKey, Option<MarineReport>>,
}

impl OpenMeteoProvider {
    pub fn new(client: reqwest::Client) -> Self {
        Self::with_urls(client, FORECAST_URL, MARINE_URL)
    }

    pub fn with_urls(
        client: reqwest::Client,
        forecast_url: impl Into<String>,
        marine_url: impl Into<String>,
    ) -> Self {
        Self {
            client,
            forecast_url: forecast_url.into(),
            marine_url: marine_url.into(),
            forecasts: TtlCache::new(FORECAST_TTL, 256),
            marine: TtlCache::new(MARINE_TTL, 256),
        }
    }

    async fn fetch_forecast(
        &self,
        point: GeoPoint,
        range: ForecastRange,
    ) -> Result<Arc<Fetched<OmForecast>>, ProviderError> {
        let range = ForecastRange {
            hours: range.hours.clamp(1, 384),
            days: range.days.clamp(1, 16),
        };
        let key = (cell(point), range);
        if let Some(hit) = self.forecasts.get(&key) {
            return Ok(hit);
        }

        let (lat, lon) = cell_coords(key.0);
        let req = self.client.get(&self.forecast_url).query(&[
            ("latitude", lat.as_str()),
            ("longitude", lon.as_str()),
            ("current", CURRENT_VARS),
            ("hourly", HOURLY_VARS),
            ("daily", DAILY_VARS),
            ("wind_speed_unit", "ms"),
            ("timezone", "auto"),
            ("timeformat", "unixtime"),
            ("forecast_hours", &range.hours.to_string()),
            ("forecast_days", &range.days.to_string()),
        ]);
        let fetched = Arc::new(Fetched {
            at: Utc::now(),
            data: get_json::<OmForecast>(ID, req).await?,
        });
        self.forecasts.insert(key, fetched.clone());
        Ok(fetched)
    }
}

fn source(at: DateTime<Utc>) -> SourceMeta {
    SourceMeta {
        provider_id: ID.to_owned(),
        fetched_at: at,
    }
}

#[async_trait]
impl WeatherProvider for OpenMeteoProvider {
    fn id(&self) -> &str {
        ID
    }

    fn name(&self) -> &str {
        "Open-Meteo"
    }

    fn capabilities(&self) -> &[Capability] {
        CAPABILITIES
    }

    async fn current(&self, point: GeoPoint) -> Result<CurrentConditions, ProviderError> {
        // Same range as the dashboard's default forecast, so both share a request.
        let f = self.fetch_forecast(point, ForecastRange::default()).await?;
        parse_current(&f.data, source(f.at))
    }

    async fn forecast(
        &self,
        point: GeoPoint,
        range: ForecastRange,
    ) -> Result<Forecast, ProviderError> {
        let f = self.fetch_forecast(point, range).await?;
        parse_forecast(&f.data, source(f.at))
    }

    async fn marine(&self, point: GeoPoint) -> Result<Option<MarineReport>, ProviderError> {
        let key = cell(point);
        if let Some(hit) = self.marine.get(&key) {
            return Ok(hit);
        }
        let (lat, lon) = cell_coords(key);
        let req = self.client.get(&self.marine_url).query(&[
            ("latitude", lat.as_str()),
            ("longitude", lon.as_str()),
            ("hourly", MARINE_VARS),
            ("timezone", "auto"),
            ("timeformat", "unixtime"),
            ("forecast_days", &MARINE_DAYS.to_string()),
        ]);
        let raw: OmMarine = get_json(ID, req).await?;
        let report = parse_marine(&raw, source(Utc::now()))?;
        self.marine.insert(key, report.clone());
        Ok(report)
    }
}

/// Maps a WMO weather interpretation code to a [`Condition`].
pub fn condition_from_wmo(code: u16) -> Condition {
    match code {
        0 | 1 => Condition::Clear,
        2 => Condition::PartlyCloudy,
        3 => Condition::Cloudy,
        45 | 48 => Condition::Fog,
        51 | 53 | 55 => Condition::Drizzle,
        56 | 57 | 66 | 67 => Condition::FreezingRain,
        61 | 63 | 65 | 80 | 81 | 82 => Condition::Rain,
        71 | 73 | 75 | 77 | 85 | 86 => Condition::Snow,
        95 | 96 | 99 => Condition::Thunderstorm,
        _ => Condition::Unknown,
    }
}

// ---- Wire format -----------------------------------------------------------

#[derive(Debug, Deserialize)]
struct OmForecast {
    utc_offset_seconds: i32,
    timezone: String,
    current: Option<OmCurrent>,
    #[serde(default)]
    hourly: OmHourly,
    #[serde(default)]
    daily: OmDaily,
}

#[derive(Debug, Deserialize)]
struct OmCurrent {
    time: i64,
    temperature_2m: Option<f64>,
    apparent_temperature: Option<f64>,
    relative_humidity_2m: Option<f64>,
    is_day: Option<u8>,
    precipitation: Option<f64>,
    weather_code: Option<u16>,
    cloud_cover: Option<f64>,
    pressure_msl: Option<f64>,
    wind_speed_10m: Option<f64>,
    wind_direction_10m: Option<f64>,
    wind_gusts_10m: Option<f64>,
    uv_index: Option<f64>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct OmHourly {
    time: Vec<i64>,
    temperature_2m: Vec<Option<f64>>,
    precipitation_probability: Vec<Option<f64>>,
    precipitation: Vec<Option<f64>>,
    weather_code: Vec<Option<u16>>,
    wind_speed_10m: Vec<Option<f64>>,
    is_day: Vec<Option<u8>>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct OmDaily {
    time: Vec<i64>,
    weather_code: Vec<Option<u16>>,
    temperature_2m_max: Vec<Option<f64>>,
    temperature_2m_min: Vec<Option<f64>>,
    precipitation_sum: Vec<Option<f64>>,
    precipitation_probability_max: Vec<Option<f64>>,
    sunrise: Vec<Option<i64>>,
    sunset: Vec<Option<i64>>,
}

#[derive(Debug, Deserialize)]
struct OmMarine {
    utc_offset_seconds: i32,
    #[serde(default)]
    hourly: OmMarineHourly,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct OmMarineHourly {
    time: Vec<i64>,
    wave_height: Vec<Option<f64>>,
    wave_direction: Vec<Option<f64>>,
    wave_period: Vec<Option<f64>>,
    swell_wave_height: Vec<Option<f64>>,
    sea_surface_temperature: Vec<Option<f64>>,
    sea_level_height_msl: Vec<Option<f64>>,
}

// ---- Normalization ---------------------------------------------------------

fn bad(message: impl Into<String>) -> ProviderError {
    ProviderError::BadResponse {
        provider: ID.to_owned(),
        message: message.into(),
    }
}

fn offset(secs: i32) -> Result<FixedOffset, ProviderError> {
    FixedOffset::east_opt(secs).ok_or_else(|| bad(format!("invalid utc offset {secs}")))
}

fn local(t: i64, off: FixedOffset) -> Result<DateTime<FixedOffset>, ProviderError> {
    DateTime::from_timestamp(t, 0)
        .map(|d| d.with_timezone(&off))
        .ok_or_else(|| bad(format!("timestamp {t} out of range")))
}

/// Value at `i` of a column that may be shorter than `time` or contain nulls.
fn at<T: Copy>(col: &[Option<T>], i: usize) -> Option<T> {
    col.get(i).copied().flatten()
}

fn condition(code: Option<u16>) -> Condition {
    code.map(condition_from_wmo).unwrap_or(Condition::Unknown)
}

fn parse_current(f: &OmForecast, source: SourceMeta) -> Result<CurrentConditions, ProviderError> {
    let c = f.current.as_ref().ok_or_else(|| bad("missing current block"))?;
    let off = offset(f.utc_offset_seconds)?;
    Ok(CurrentConditions {
        observed_at: local(c.time, off)?,
        temperature_c: c.temperature_2m.ok_or_else(|| bad("missing temperature"))?,
        feels_like_c: c.apparent_temperature,
        humidity_pct: c.relative_humidity_2m,
        wind_speed_ms: c.wind_speed_10m,
        wind_gust_ms: c.wind_gusts_10m,
        wind_dir_deg: c.wind_direction_10m,
        pressure_hpa: c.pressure_msl,
        precip_mm: c.precipitation,
        cloud_cover_pct: c.cloud_cover,
        uv_index: c.uv_index,
        condition: condition(c.weather_code),
        is_day: c.is_day.unwrap_or(1) == 1,
        source,
    })
}

fn parse_forecast(f: &OmForecast, source: SourceMeta) -> Result<Forecast, ProviderError> {
    let off = offset(f.utc_offset_seconds)?;
    let h = &f.hourly;
    let d = &f.daily;

    let mut hourly = Vec::with_capacity(h.time.len());
    for (i, &t) in h.time.iter().enumerate() {
        let Some(temperature_c) = at(&h.temperature_2m, i) else { continue };
        hourly.push(HourlyPoint {
            time: local(t, off)?,
            temperature_c,
            precip_prob_pct: at(&h.precipitation_probability, i),
            precip_mm: at(&h.precipitation, i),
            wind_speed_ms: at(&h.wind_speed_10m, i),
            condition: condition(at(&h.weather_code, i)),
            is_day: at(&h.is_day, i).unwrap_or(1) == 1,
        });
    }

    let mut daily = Vec::with_capacity(d.time.len());
    for (i, &t) in d.time.iter().enumerate() {
        let (Some(min_c), Some(max_c)) = (at(&d.temperature_2m_min, i), at(&d.temperature_2m_max, i))
        else {
            continue;
        };
        daily.push(DailyPoint {
            // Daily timestamps are local midnight.
            date: local(t, off)?.date_naive(),
            min_c,
            max_c,
            precip_mm: at(&d.precipitation_sum, i),
            precip_prob_pct: at(&d.precipitation_probability_max, i),
            sunrise: at(&d.sunrise, i).map(|s| local(s, off)).transpose()?,
            sunset: at(&d.sunset, i).map(|s| local(s, off)).transpose()?,
            condition: condition(at(&d.weather_code, i)),
        });
    }

    Ok(Forecast {
        timezone: f.timezone.clone(),
        hourly,
        daily,
        source,
    })
}

/// `Ok(None)` for inland points: the marine API returns all-null series there.
fn parse_marine(m: &OmMarine, source: SourceMeta) -> Result<Option<MarineReport>, ProviderError> {
    let off = offset(m.utc_offset_seconds)?;
    let h = &m.hourly;
    let mut waves = Vec::new();
    let mut series = Vec::new();

    for (i, &t) in h.time.iter().enumerate() {
        let time = local(t, off)?;
        let point = WavePoint {
            time,
            wave_height_m: at(&h.wave_height, i),
            wave_period_s: at(&h.wave_period, i),
            wave_dir_deg: at(&h.wave_direction, i),
            swell_height_m: at(&h.swell_wave_height, i),
            sea_temp_c: at(&h.sea_surface_temperature, i),
        };
        if point.wave_height_m.is_some() || point.swell_height_m.is_some() || point.sea_temp_c.is_some() {
            waves.push(point);
        }
        if let Some(height_m) = at(&h.sea_level_height_msl, i) {
            series.push(TideSample { time, height_m });
        }
    }

    if waves.is_empty() && series.is_empty() {
        return Ok(None);
    }
    let tides = (series.len() >= 3).then(|| TideData {
        datum: "MSL".into(),
        modeled: true,
        events: tides::find_extrema(&series),
        series,
    });
    Ok(Some(MarineReport { waves, tides, source }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn src() -> SourceMeta {
        source(Utc::now())
    }

    fn forecast_fixture() -> OmForecast {
        serde_json::from_str(include_str!("../../fixtures/open_meteo_forecast_amsterdam.json")).unwrap()
    }

    fn marine_fixture(name: &str) -> OmMarine {
        let path = format!("{}/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn parses_current_conditions() {
        let c = parse_current(&forecast_fixture(), src()).unwrap();
        assert_eq!(c.observed_at.offset().local_minus_utc(), 3600);
        assert!((-30.0..45.0).contains(&c.temperature_c));
        assert!(c.wind_speed_ms.is_some() && c.pressure_hpa.is_some());
        assert_ne!(c.condition, Condition::Unknown);
    }

    #[test]
    fn parses_hourly_and_daily() {
        let f = parse_forecast(&forecast_fixture(), src()).unwrap();
        assert_eq!(f.timezone, "Europe/Amsterdam");
        assert_eq!(f.hourly.len(), 48);
        assert_eq!(f.daily.len(), 7);
        assert!(f.hourly.windows(2).all(|w| w[1].time > w[0].time));
        let today = &f.daily[0];
        assert!(today.min_c <= today.max_c);
        // Daily timestamps are local midnight, so the date must match the local sunrise date.
        assert_eq!(today.sunrise.unwrap().date_naive(), today.date);
    }

    #[test]
    fn coastal_marine_has_waves_and_tides() {
        let m = parse_marine(&marine_fixture("open_meteo_marine_scheveningen.json"), src())
            .unwrap()
            .expect("coastal");
        assert!(!m.waves.is_empty());
        let tides = m.tides.expect("tides");
        assert!(tides.modeled);
        // North Sea: semidiurnal, so ~2 highs and ~2 lows per day over 2 days.
        assert!((6..=9).contains(&tides.events.len()), "{:?}", tides.events);
        assert!(tides.events.windows(2).all(|w| w[0].kind != w[1].kind));
    }

    #[test]
    fn lake_has_waves_but_no_tides() {
        let m = parse_marine(&marine_fixture("open_meteo_marine_chicago_lake.json"), src())
            .unwrap()
            .expect("lake has marine data");
        assert!(!m.waves.is_empty());
        assert!(m.tides.is_none());
    }

    #[test]
    fn inland_has_no_marine_report() {
        let m = parse_marine(&marine_fixture("open_meteo_marine_utrecht_inland.json"), src()).unwrap();
        assert!(m.is_none());
    }

    #[test]
    fn maps_wmo_codes() {
        assert_eq!(condition_from_wmo(0), Condition::Clear);
        assert_eq!(condition_from_wmo(3), Condition::Cloudy);
        assert_eq!(condition_from_wmo(66), Condition::FreezingRain);
        assert_eq!(condition_from_wmo(82), Condition::Rain);
        assert_eq!(condition_from_wmo(99), Condition::Thunderstorm);
        assert_eq!(condition_from_wmo(42), Condition::Unknown);
    }

    /// Hits the real API: `cargo test -p weather-core -- --ignored`.
    #[tokio::test]
    #[ignore]
    async fn live_open_meteo() {
        let p = OpenMeteoProvider::new(crate::http::default_client());
        let c = p.current(GeoPoint::new(52.37, 4.90)).await.unwrap();
        assert_eq!(c.source.provider_id, ID);
        let f = p.forecast(GeoPoint::new(52.37, 4.90), ForecastRange::default()).await.unwrap();
        assert_eq!(f.hourly.len(), 48);
        let m = p.marine(GeoPoint::new(52.11, 4.27)).await.unwrap().unwrap();
        assert!(m.tides.unwrap().events.len() >= 4);
        assert!(p.marine(GeoPoint::new(39.74, -104.99)).await.unwrap().is_none());
    }
}
