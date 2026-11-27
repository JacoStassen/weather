//! MET Norway's Locationforecast (<https://api.met.no>): the forecast behind
//! Yr. Global, free (CC BY 4.0), no API key; MET asks clients to send an
//! identifying User-Agent and not to poll more often than the data changes
//! (about every 30 minutes).
//!
//! The API answers in UTC without a time zone. The zone (for local days) comes
//! from Open-Meteo, which knows it per coordinate; sunrise and sunset, which
//! MET doesn't send, are calculated.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Datelike, NaiveDate, TimeDelta, Utc};
use chrono_tz::Tz;
use serde::Deserialize;

use crate::cache::TtlCache;
use crate::error::ProviderError;
use crate::http::get_json;
use crate::provider::WeatherProvider;
use crate::providers::open_meteo;
use crate::types::*;

pub const ID: &str = "yr";
pub const FORECAST_URL: &str = "https://api.met.no/weatherapi/locationforecast/2.0/complete";

const CAPABILITIES: &[Capability] = &[Capability::Current, Capability::Hourly, Capability::Daily];

/// MET updates its forecast about twice an hour and says so in `Expires`.
const FORECAST_TTL: Duration = Duration::from_secs(30 * 60);
const ZONE_TTL: Duration = Duration::from_secs(24 * 3600);

type CellKey = (i32, i32);

/// 0.01° (~1 km): the cache key, and the precision sent to MET (it asks for at most 4 decimals).
fn cell(p: GeoPoint) -> CellKey {
    ((p.lat * 100.0).round() as i32, (p.lon * 100.0).round() as i32)
}

fn cell_coords(k: CellKey) -> (String, String) {
    (
        format!("{:.2}", k.0 as f64 / 100.0),
        format!("{:.2}", k.1 as f64 / 100.0),
    )
}

struct Fetched {
    at: DateTime<Utc>,
    data: MetResponse,
}

pub struct MetNoProvider {
    client: reqwest::Client,
    forecast_url: String,
    zone_url: String,
    forecasts: TtlCache<CellKey, Arc<Fetched>>,
    zones: TtlCache<CellKey, Tz>,
}

impl MetNoProvider {
    pub fn new(client: reqwest::Client) -> Self {
        Self::with_urls(client, FORECAST_URL, open_meteo::FORECAST_URL)
    }

    pub fn with_urls(client: reqwest::Client, forecast_url: impl Into<String>, zone_url: impl Into<String>) -> Self {
        Self {
            client,
            forecast_url: forecast_url.into(),
            zone_url: zone_url.into(),
            forecasts: TtlCache::new(FORECAST_TTL, 256),
            zones: TtlCache::new(ZONE_TTL, 1024),
        }
    }

    async fn fetch(&self, point: GeoPoint) -> Result<Arc<Fetched>, ProviderError> {
        let key = cell(point);
        if let Some(hit) = self.forecasts.get(&key) {
            return Ok(hit);
        }
        let (lat, lon) = cell_coords(key);
        let req = self
            .client
            .get(&self.forecast_url)
            .query(&[("lat", lat.as_str()), ("lon", lon.as_str())]);
        let fetched = Arc::new(Fetched {
            at: Utc::now(),
            data: get_json(ID, req).await?,
        });
        self.forecasts.insert(key, fetched.clone());
        Ok(fetched)
    }

    /// The location's time zone. If the lookup fails, a whole-hour zone from
    /// the longitude stands in (not cached, so the next request tries again).
    async fn zone(&self, point: GeoPoint) -> Tz {
        #[derive(Deserialize)]
        struct Zone {
            timezone: String,
        }
        let key = cell(point);
        if let Some(tz) = self.zones.get(&key) {
            return tz;
        }
        let (lat, lon) = cell_coords(key);
        let req = self.client.get(&self.zone_url).query(&[
            ("latitude", lat.as_str()),
            ("longitude", lon.as_str()),
            ("timezone", "auto"),
            ("forecast_days", "1"),
        ]);
        match get_json::<Zone>(ID, req).await.ok().and_then(|z| z.timezone.parse::<Tz>().ok()) {
            Some(tz) => {
                self.zones.insert(key, tz);
                tz
            }
            None => zone_from_longitude(point.lon),
        }
    }

    fn source(at: DateTime<Utc>) -> SourceMeta {
        SourceMeta {
            provider_id: ID.to_owned(),
            fetched_at: at,
        }
    }
}

#[async_trait]
impl WeatherProvider for MetNoProvider {
    fn id(&self) -> &str {
        ID
    }

    fn name(&self) -> &str {
        "Yr · MET Norway"
    }

    fn description(&self) -> Option<&str> {
        Some(
            "MET Norway's forecast, as shown on yr.no. Outside the Nordic countries it's ECMWF's model with MET's own \
             adjustments (such as for altitude). No chance of rain outside the Nordics. Data: MET Norway, CC BY 4.0.",
        )
    }

    fn capabilities(&self) -> &[Capability] {
        CAPABILITIES
    }

    async fn current(&self, point: GeoPoint) -> Result<CurrentConditions, ProviderError> {
        let (f, tz) = futures::join!(self.fetch(point), self.zone(point));
        let f = f?;
        parse_current(&f.data, tz, point, Self::source(f.at))
    }

    async fn forecast(&self, point: GeoPoint, range: ForecastRange) -> Result<Forecast, ProviderError> {
        let (f, tz) = futures::join!(self.fetch(point), self.zone(point));
        let f = f?;
        parse_forecast(&f.data, tz, point, range, Self::source(f.at))
    }
}

/// `Etc/GMT-2` is UTC+2: the sign in those names is inverted.
fn zone_from_longitude(lon: f64) -> Tz {
    let h = (lon / 15.0).round() as i32;
    let name = match h {
        0 => "Etc/UTC".to_owned(),
        h if h > 0 => format!("Etc/GMT-{h}"),
        h => format!("Etc/GMT+{}", -h),
    };
    name.parse().unwrap_or(Tz::UTC)
}

// ---- Wire format -----------------------------------------------------------

#[derive(Debug, Deserialize)]
struct MetResponse {
    properties: MetProperties,
}

#[derive(Debug, Deserialize)]
struct MetProperties {
    timeseries: Vec<MetStep>,
}

#[derive(Debug, Deserialize)]
struct MetStep {
    time: DateTime<Utc>,
    data: MetData,
}

#[derive(Debug, Deserialize)]
struct MetData {
    instant: MetInstant,
    next_1_hours: Option<MetPeriod>,
    next_6_hours: Option<MetPeriod>,
    next_12_hours: Option<MetPeriod>,
}

#[derive(Debug, Deserialize)]
struct MetInstant {
    #[serde(default)]
    details: MetInstantDetails,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct MetInstantDetails {
    air_temperature: Option<f64>,
    apparent_air_temperature: Option<f64>,
    relative_humidity: Option<f64>,
    wind_speed: Option<f64>,
    wind_speed_of_gust: Option<f64>,
    wind_from_direction: Option<f64>,
    air_pressure_at_sea_level: Option<f64>,
    cloud_area_fraction: Option<f64>,
    ultraviolet_index_clear_sky: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct MetPeriod {
    summary: Option<MetSummary>,
    #[serde(default)]
    details: MetPeriodDetails,
}

#[derive(Debug, Deserialize)]
struct MetSummary {
    symbol_code: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct MetPeriodDetails {
    precipitation_amount: Option<f64>,
    probability_of_precipitation: Option<f64>,
    air_temperature_max: Option<f64>,
    air_temperature_min: Option<f64>,
}

impl MetData {
    /// The shortest period that has data, with its length in hours.
    fn period(&self) -> Option<(&MetPeriod, i64)> {
        self.next_1_hours
            .as_ref()
            .map(|p| (p, 1))
            .or(self.next_6_hours.as_ref().map(|p| (p, 6)))
            .or(self.next_12_hours.as_ref().map(|p| (p, 12)))
    }
}

impl MetPeriod {
    fn symbol(&self) -> Option<&str> {
        self.summary.as_ref().map(|s| s.symbol_code.as_str())
    }
}

// ---- Normalization ---------------------------------------------------------

fn bad(message: impl Into<String>) -> ProviderError {
    ProviderError::BadResponse {
        provider: ID.to_owned(),
        message: message.into(),
    }
}

/// Maps a Yr symbol ("lightrainshowers_day") to a condition, and whether it's
/// the day variant (None when the symbol has no day/night form).
pub fn condition_from_symbol(code: &str) -> (Condition, Option<bool>) {
    let (base, variant) = match code.rsplit_once('_') {
        Some((b, v)) => (b, Some(v)),
        None => (code, None),
    };
    let condition = if base.contains("thunder") {
        Condition::Thunderstorm
    } else if base.contains("sleet") {
        Condition::Sleet
    } else if base.contains("snow") {
        Condition::Snow
    } else if base.starts_with("lightrain") {
        Condition::Drizzle
    } else if base.contains("rain") {
        Condition::Rain
    } else {
        match base {
            "clearsky" | "fair" => Condition::Clear,
            "partlycloudy" => Condition::PartlyCloudy,
            "cloudy" => Condition::Cloudy,
            "fog" => Condition::Fog,
            _ => Condition::Unknown,
        }
    };
    (condition, variant.map(|v| v == "day"))
}

/// How much a condition should dominate a day's summary (the day shows its worst).
fn severity(c: Condition) -> i8 {
    match c {
        Condition::Thunderstorm => 9,
        Condition::Snow => 8,
        Condition::Sleet => 7,
        Condition::FreezingRain => 6,
        Condition::Rain => 5,
        Condition::Drizzle => 4,
        Condition::Fog => 3,
        Condition::Cloudy => 2,
        Condition::PartlyCloudy => 1,
        Condition::Clear => 0,
        Condition::Unknown => -1,
    }
}

fn is_day_at(t: DateTime<Utc>, tz: Tz, point: GeoPoint) -> bool {
    match sun_times(t.with_timezone(&tz).date_naive(), point) {
        Some((rise, set)) => t >= rise && t < set,
        // Polar day or night: the sun is up all day if it's summer at that pole.
        None => (point.lat > 0.0) == (4..=9).contains(&t.with_timezone(&tz).month()),
    }
}

fn parse_current(r: &MetResponse, tz: Tz, point: GeoPoint, source: SourceMeta) -> Result<CurrentConditions, ProviderError> {
    let step = r.properties.timeseries.first().ok_or_else(|| bad("empty timeseries"))?;
    let i = &step.data.instant.details;
    let temperature_c = i.air_temperature.ok_or_else(|| bad("no current temperature"))?;
    let period = step.data.period();
    let (condition, day) = period
        .and_then(|(p, _)| p.symbol())
        .map(condition_from_symbol)
        .unwrap_or((Condition::Unknown, None));
    Ok(CurrentConditions {
        observed_at: step.time.with_timezone(&tz).fixed_offset(),
        temperature_c,
        feels_like_c: i.apparent_air_temperature,
        humidity_pct: i.relative_humidity,
        wind_speed_ms: i.wind_speed,
        wind_gust_ms: i.wind_speed_of_gust,
        wind_dir_deg: i.wind_from_direction,
        pressure_hpa: i.air_pressure_at_sea_level,
        precip_mm: step.data.next_1_hours.as_ref().and_then(|p| p.details.precipitation_amount),
        cloud_cover_pct: i.cloud_area_fraction,
        uv_index: i.ultraviolet_index_clear_sky,
        condition,
        is_day: day.unwrap_or_else(|| is_day_at(step.time, tz, point)),
        source,
    })
}

#[derive(Default)]
struct DayAcc {
    min: Option<f64>,
    max: Option<f64>,
    precip: Option<f64>,
    pop: Option<f64>,
    condition: Option<Condition>,
    steps: usize,
}

impl DayAcc {
    fn temp(&mut self, t: f64) {
        self.min = Some(self.min.map_or(t, |m| m.min(t)));
        self.max = Some(self.max.map_or(t, |m| m.max(t)));
    }
}

fn parse_forecast(
    r: &MetResponse,
    tz: Tz,
    point: GeoPoint,
    range: ForecastRange,
    source: SourceMeta,
) -> Result<Forecast, ProviderError> {
    let steps = &r.properties.timeseries;
    if steps.is_empty() {
        return Err(bad("empty timeseries"));
    }

    // Hourly: the first ~60 hours have 1-hour periods.
    let hourly = steps
        .iter()
        .filter_map(|s| {
            let p = s.data.next_1_hours.as_ref()?;
            let temperature_c = s.data.instant.details.air_temperature?;
            let (condition, day) = p.symbol().map(condition_from_symbol).unwrap_or((Condition::Unknown, None));
            Some(HourlyPoint {
                time: s.time.with_timezone(&tz).fixed_offset(),
                temperature_c,
                precip_prob_pct: p.details.probability_of_precipitation,
                precip_mm: p.details.precipitation_amount,
                wind_speed_ms: s.data.instant.details.wind_speed,
                condition,
                is_day: day.unwrap_or_else(|| is_day_at(s.time, tz, point)),
            })
        })
        .take(range.hours as usize)
        .collect();

    // Daily: summarise each local day. Later steps are 6 hours apart; their
    // periods carry the min/max between the steps. Rain is added once per
    // stretch of time, so overlapping 1- and 6-hour periods don't count twice.
    let mut days: BTreeMap<NaiveDate, DayAcc> = BTreeMap::new();
    let mut counted_until = DateTime::<Utc>::MIN_UTC;
    for s in steps {
        let day = days.entry(s.time.with_timezone(&tz).date_naive()).or_default();
        day.steps += 1;
        if let Some(t) = s.data.instant.details.air_temperature {
            day.temp(t);
        }
        let Some((p, hours)) = s.data.period() else { continue };
        if hours > 1 {
            for t in [p.details.air_temperature_max, p.details.air_temperature_min].into_iter().flatten() {
                day.temp(t);
            }
        }
        if s.time >= counted_until {
            if let Some(mm) = p.details.precipitation_amount {
                day.precip = Some(day.precip.unwrap_or(0.0) + mm);
            }
            counted_until = s.time + TimeDelta::hours(hours);
        }
        if let Some(pop) = p.details.probability_of_precipitation {
            day.pop = Some(day.pop.map_or(pop, |m: f64| m.max(pop)));
        }
        if let Some((c, _)) = p.symbol().map(condition_from_symbol) {
            if day.condition.is_none_or(|d| severity(c) > severity(d)) {
                day.condition = Some(c);
            }
        }
    }

    let first = days.keys().next().copied();
    let daily = days
        .into_iter()
        // Today counts from now; the last day is often only a step or two, too little to summarise.
        .filter(|(date, d)| Some(*date) == first || d.steps >= 4)
        .filter_map(|(date, d)| {
            let (min_c, max_c) = (d.min?, d.max?);
            let sun = sun_times(date, point);
            Some(DailyPoint {
                date,
                min_c,
                max_c,
                precip_mm: d.precip.map(|mm| (mm * 10.0).round() / 10.0),
                precip_prob_pct: d.pop,
                sunrise: sun.map(|(r, _)| r.with_timezone(&tz).fixed_offset()),
                sunset: sun.map(|(_, s)| s.with_timezone(&tz).fixed_offset()),
                condition: d.condition.unwrap_or(Condition::Unknown),
            })
        })
        .take(range.days as usize)
        .collect();

    Ok(Forecast {
        timezone: tz.name().to_owned(),
        hourly,
        daily,
        source,
    })
}

/// Sunrise and sunset (UTC) on a local date, from the standard sunrise
/// equation; within a couple of minutes. None during polar day or night.
pub fn sun_times(date: NaiveDate, p: GeoPoint) -> Option<(DateTime<Utc>, DateTime<Utc>)> {
    let rad = f64::to_radians;
    let j2000 = NaiveDate::from_ymd_opt(2000, 1, 1)?;
    let n = (date - j2000).num_days() as f64;
    let j_star = n - p.lon / 360.0;
    let m = (357.5291 + 0.985_600_28 * j_star).rem_euclid(360.0);
    let c = 1.9148 * rad(m).sin() + 0.02 * rad(2.0 * m).sin() + 0.0003 * rad(3.0 * m).sin();
    let lambda = (m + c + 180.0 + 102.9372).rem_euclid(360.0);
    let transit = 2_451_545.0 + j_star + 0.0053 * rad(m).sin() - 0.0069 * rad(2.0 * lambda).sin();
    let decl = (rad(lambda).sin() * rad(23.4397).sin()).asin();
    let cos_w = (rad(-0.833).sin() - rad(p.lat).sin() * decl.sin()) / (rad(p.lat).cos() * decl.cos());
    if !(-1.0..=1.0).contains(&cos_w) {
        return None;
    }
    let w = cos_w.acos().to_degrees() / 360.0;
    let to_utc = |jd: f64| DateTime::from_timestamp(((jd - 2_440_587.5) * 86_400.0).round() as i64, 0);
    Some((to_utc(transit - w)?, to_utc(transit + w)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{NaiveTime, Timelike};

    const CAPE_TOWN: GeoPoint = GeoPoint { lat: -33.93, lon: 18.42 };

    fn fixture() -> MetResponse {
        serde_json::from_str(include_str!("../../fixtures/met_locationforecast_cape_town.json")).unwrap()
    }

    fn src() -> SourceMeta {
        MetNoProvider::source(Utc::now())
    }

    fn joburg() -> Tz {
        "Africa/Johannesburg".parse().unwrap()
    }

    #[test]
    fn maps_symbols() {
        assert_eq!(condition_from_symbol("clearsky_day"), (Condition::Clear, Some(true)));
        assert_eq!(condition_from_symbol("fair_night"), (Condition::Clear, Some(false)));
        assert_eq!(condition_from_symbol("partlycloudy_polartwilight"), (Condition::PartlyCloudy, Some(false)));
        assert_eq!(condition_from_symbol("cloudy"), (Condition::Cloudy, None));
        assert_eq!(condition_from_symbol("fog").0, Condition::Fog);
        assert_eq!(condition_from_symbol("lightrainshowers_day").0, Condition::Drizzle);
        assert_eq!(condition_from_symbol("heavyrain").0, Condition::Rain);
        assert_eq!(condition_from_symbol("rainshowersandthunder_day").0, Condition::Thunderstorm);
        assert_eq!(condition_from_symbol("lightsleet").0, Condition::Sleet);
        assert_eq!(condition_from_symbol("heavysnowshowers_night").0, Condition::Snow);
        assert_eq!(condition_from_symbol("something_new").0, Condition::Unknown);
    }

    #[test]
    fn parses_current_conditions() {
        let c = parse_current(&fixture(), joburg(), CAPE_TOWN, src()).unwrap();
        assert_eq!(c.temperature_c, 15.8);
        assert_eq!(c.feels_like_c, Some(15.8));
        assert_eq!(c.humidity_pct, Some(92.2));
        assert_eq!(c.pressure_hpa, Some(1007.7));
        assert_eq!(c.condition, Condition::Clear);
        assert!(!c.is_day, "01:00 UTC is 03:00 in Cape Town");
        assert_eq!(c.observed_at.offset().local_minus_utc(), 2 * 3600);
        assert_eq!(c.source.provider_id, ID);
    }

    #[test]
    fn parses_hourly_and_daily() {
        let f = parse_forecast(&fixture(), joburg(), CAPE_TOWN, ForecastRange::default(), src()).unwrap();
        assert_eq!(f.timezone, "Africa/Johannesburg");
        assert_eq!(f.hourly.len(), 48);
        let gaps: Vec<_> = f.hourly.windows(2).map(|w| (w[1].time - w[0].time).num_hours()).collect();
        assert!(gaps.iter().all(|&h| h == 1), "hourly steps: {gaps:?}");

        assert_eq!(f.daily.len(), 7);
        assert_eq!(f.daily[0].date, NaiveDate::from_ymd_opt(2026, 11, 27).unwrap());
        let dates: Vec<_> = f.daily.windows(2).map(|w| (w[1].date - w[0].date).num_days()).collect();
        assert!(dates.iter().all(|&d| d == 1));
        for d in &f.daily {
            assert!(d.min_c < d.max_c && (5.0..40.0).contains(&d.max_c), "{d:?}");
            assert!(d.precip_mm.is_some_and(|p| p >= 0.0));
            assert_ne!(d.condition, Condition::Unknown);
            let rise = d.sunrise.unwrap().time();
            assert!(rise > NaiveTime::from_hms_opt(5, 0, 0).unwrap() && rise < NaiveTime::from_hms_opt(6, 0, 0).unwrap());
        }
    }

    #[test]
    fn rain_is_counted_once_where_periods_overlap() {
        let f = parse_forecast(&fixture(), joburg(), CAPE_TOWN, ForecastRange { hours: 240, days: 16 }, src()).unwrap();
        let r = fixture();
        // Upper bound: every 1-hour amount plus every 6-hour amount from steps without 1-hour data.
        let total: f64 = r
            .properties
            .timeseries
            .iter()
            .filter_map(|s| match (&s.data.next_1_hours, &s.data.next_6_hours) {
                (Some(p), _) => p.details.precipitation_amount,
                (None, Some(p)) => p.details.precipitation_amount,
                _ => None,
            })
            .sum();
        let daily: f64 = f.daily.iter().filter_map(|d| d.precip_mm).sum();
        assert!(daily <= total + 1e-9, "daily {daily} > {total}");
    }

    #[test]
    fn sunrise_and_sunset() {
        // Cape Town, 27 Nov: about 05:29 and 19:39 SAST.
        let (rise, set) = sun_times(NaiveDate::from_ymd_opt(2026, 11, 27).unwrap(), CAPE_TOWN).unwrap();
        let (rise, set) = (rise.with_timezone(&joburg()), set.with_timezone(&joburg()));
        assert_eq!((rise.hour(), set.hour()), (5, 19));
        assert!((25..=33).contains(&rise.minute()), "{rise}");
        assert!((35..=43).contains(&set.minute()), "{set}");
        // Tromsø in December: polar night.
        assert!(sun_times(NaiveDate::from_ymd_opt(2026, 12, 21).unwrap(), GeoPoint::new(69.65, 18.96)).is_none());
    }

    #[test]
    fn zone_from_longitude_has_the_right_sign() {
        assert_eq!(zone_from_longitude(18.4).name(), "Etc/GMT-1");
        assert_eq!(zone_from_longitude(-74.0).name(), "Etc/GMT+5");
        assert_eq!(zone_from_longitude(2.0).name(), "Etc/UTC");
    }

    #[tokio::test]
    #[ignore = "hits the network"]
    async fn live_met_no() {
        let p = MetNoProvider::new(crate::http::default_client());
        let c = p.current(CAPE_TOWN).await.unwrap();
        assert_eq!(c.source.provider_id, ID);
        let f = p.forecast(CAPE_TOWN, ForecastRange::default()).await.unwrap();
        assert_eq!(f.timezone, "Africa/Johannesburg");
        assert!(f.daily.len() >= 7, "{} days", f.daily.len());
        assert_eq!(f.hourly.len(), 48);
    }
}
