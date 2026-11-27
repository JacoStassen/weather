//! How accurate each forecast source has been, per place, measured against
//! real thermometer readings. The blend source weighs the sources by it.
//!
//! Every hour, for each place people look at, the server notes what each
//! source predicts 6, 24 and 48 hours ahead at the nearest airport weather
//! station (METAR reports, via aviationweather.gov: worldwide, free, hourly).
//! When the hour comes, the prediction is compared with the station's reading
//! and the source's running error is updated. Only temperature is scored:
//! it's measured everywhere and comparable between sources.
//!
//! Forecasts are taken for the station's own coordinates, so the scores judge
//! the models rather than the distance between the station and the place.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use chrono::{DateTime, DurationRound, TimeDelta, Utc};
use futures::future::join_all;
use serde::{Deserialize, Serialize};
use schemars::JsonSchema;
use ts_rs::TS;

use crate::error::ProviderError;
use crate::http::get_json;
use crate::provider::WeatherProvider;
use crate::types::{ForecastRange, GeoPoint};

pub const METAR_URL: &str = "https://aviationweather.gov/api/data";

/// Hours ahead at which predictions are recorded.
pub const LEADS: [i64; 3] = [6, 24, 48];
/// Scored predictions needed before a source's own record replaces its starting estimate.
pub const MIN_SAMPLES: u32 = 24;
/// A station farther than this doesn't represent the place.
const MAX_STATION_KM: f64 = 50.0;
/// Places nobody has looked at for this long stop being followed.
const FORGET_AFTER: TimeDelta = TimeDelta::days(14);
const MAX_PLACES: usize = 20;
/// The running error leans on roughly the last this-many readings (about a week).
const MEMORY: f64 = 500.0;

/// Starting estimates of typical 1–2 day temperature error (°C), used until a
/// source has its own record at a place. Rough, from published global
/// verification: ECMWF leads, GFS and GEM trail.
pub fn prior_mae(source: &str) -> f64 {
    match source {
        "ecmwf" => 1.5,
        "yr" | "ukmo" => 1.6,
        "icon" => 1.8,
        "gfs" => 1.9,
        _ => 2.0,
    }
}

/// Inverse-square weighting: a source half as wrong counts about four times as much.
/// The small constant keeps one lucky source from taking everything.
fn raw_weight(mae: f64) -> f64 {
    1.0 / (mae * mae + 0.25)
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Station {
    pub id: String,
    pub name: String,
    pub lat: f64,
    pub lon: f64,
    pub distance_km: f64,
}

/// Running error of one source at one place.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct Score {
    pub samples: u32,
    /// Mean absolute error, °C.
    pub mae: f64,
    /// Mean of forecast − reading, °C: positive means it runs warm.
    pub bias: f64,
}

impl Score {
    fn add(&mut self, error: f64) {
        self.samples += 1;
        let a = 1.0 / (self.samples as f64).min(MEMORY);
        self.mae += a * (error.abs() - self.mae);
        self.bias += a * (error - self.bias);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Prediction {
    source: String,
    target: DateTime<Utc>,
    lead_h: i64,
    temp_c: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Place {
    point: GeoPoint,
    last_requested: DateTime<Utc>,
    station: Option<Station>,
    station_checked: Option<DateTime<Utc>>,
    #[serde(default)]
    pending: Vec<Prediction>,
    #[serde(default)]
    scores: BTreeMap<String, Score>,
    last_reading: Option<DateTime<Utc>>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct State {
    places: Vec<Place>,
}

/// One source's share of the blend at a place.
#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SourceWeight {
    pub id: String,
    /// Share of the blend, 0–1.
    pub weight: f64,
    /// Typical temperature error (°C): measured, or the starting estimate.
    pub mae: f64,
    /// Measured mean of forecast − reading (°C), once there are enough readings.
    pub bias: Option<f64>,
    pub samples: u32,
    /// Whether `mae` is this place's own record rather than the starting estimate.
    pub measured: bool,
}

/// What the blend is based on at a place.
#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct BlendReport {
    /// The station the sources are scored against (None: none found nearby yet).
    pub station: Option<Station>,
    pub last_reading: Option<DateTime<Utc>>,
    pub sources: Vec<SourceWeight>,
}

pub struct Verifier {
    state: Mutex<State>,
    client: reqwest::Client,
    base_url: String,
}

fn km(a: GeoPoint, b: GeoPoint) -> f64 {
    let (la1, la2) = (a.lat.to_radians(), b.lat.to_radians());
    let dla = la2 - la1;
    let dlo = (b.lon - a.lon).to_radians();
    let h = (dla / 2.0).sin().powi(2) + la1.cos() * la2.cos() * (dlo / 2.0).sin().powi(2);
    6371.0 * 2.0 * h.sqrt().asin()
}

/// Same place for our purposes: within ~2 km.
const SAME_PLACE_KM: f64 = 2.0;
/// A place's scores also stand for points this close to it.
const NEARBY_KM: f64 = 30.0;

impl Verifier {
    pub fn new(client: reqwest::Client) -> Self {
        Self::with_url(client, METAR_URL)
    }

    pub fn with_url(client: reqwest::Client, base_url: impl Into<String>) -> Self {
        Self {
            state: Mutex::default(),
            client,
            base_url: base_url.into(),
        }
    }

    /// Restores saved state; unreadable state starts over.
    pub fn load(&self, json: &str) -> Result<(), serde_json::Error> {
        *self.state.lock().expect("verifier lock") = serde_json::from_str(json)?;
        Ok(())
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(&*self.state.lock().expect("verifier lock")).expect("state serializes")
    }

    /// Notes that someone looked at the weather for `point`, so it gets followed.
    pub fn track(&self, point: GeoPoint, now: DateTime<Utc>) {
        if !point.is_valid() {
            return;
        }
        let mut s = self.state.lock().expect("verifier lock");
        match s.places.iter_mut().find(|p| km(p.point, point) < SAME_PLACE_KM) {
            Some(p) => p.last_requested = now,
            None => s.places.push(Place {
                // ~1 km precision is all that's needed, and all that's stored.
                point: GeoPoint::new((point.lat * 100.0).round() / 100.0, (point.lon * 100.0).round() / 100.0),
                last_requested: now,
                station: None,
                station_checked: None,
                pending: Vec::new(),
                scores: BTreeMap::new(),
                last_reading: None,
            }),
        }
    }

    /// The blend's weights at `point` for the given sources.
    pub fn report(&self, point: GeoPoint, sources: &[&str]) -> BlendReport {
        let s = self.state.lock().expect("verifier lock");
        let place = s
            .places
            .iter()
            .filter(|p| p.station.is_some())
            .map(|p| (km(p.point, point), p))
            .filter(|(d, _)| *d < NEARBY_KM)
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, p)| p);

        let mut rows: Vec<SourceWeight> = sources
            .iter()
            .map(|&id| {
                let score = place.and_then(|p| p.scores.get(id)).copied().unwrap_or_default();
                let measured = score.samples >= MIN_SAMPLES;
                SourceWeight {
                    id: id.to_owned(),
                    weight: 0.0,
                    mae: if measured { score.mae } else { prior_mae(id) },
                    bias: measured.then_some(score.bias),
                    samples: score.samples,
                    measured,
                }
            })
            .collect();
        let total: f64 = rows.iter().map(|r| raw_weight(r.mae)).sum();
        for r in &mut rows {
            r.weight = raw_weight(r.mae) / total;
        }
        BlendReport {
            station: place.and_then(|p| p.station.clone()),
            last_reading: place.and_then(|p| p.last_reading),
            sources: rows,
        }
    }

    /// One round: forget stale places, find stations, record predictions and
    /// score the ones whose time has come. Run about hourly.
    pub async fn tick(&self, sources: &[Arc<dyn WeatherProvider>], now: DateTime<Utc>) {
        let places: Vec<(usize, Place)> = {
            let mut s = self.state.lock().expect("verifier lock");
            s.places.retain(|p| now - p.last_requested < FORGET_AFTER);
            s.places.sort_by_key(|p| std::cmp::Reverse(p.last_requested));
            s.places.truncate(MAX_PLACES);
            s.places.iter().cloned().enumerate().collect()
        };

        for (i, mut place) in places {
            self.refresh_station(&mut place, now).await;
            if let Some(station) = place.station.clone() {
                self.predict(&mut place, &station, sources, now).await;
                match self.readings(&station.id).await {
                    Ok(readings) => score(&mut place, &readings, now),
                    Err(e) => tracing_warn(&format!("readings for {}: {e}", station.id)),
                }
            }
            // Write back unless the place was dropped meanwhile; keep any newer request time.
            let mut s = self.state.lock().expect("verifier lock");
            if let Some(slot) = s.places.get_mut(i).filter(|p| km(p.point, place.point) < 0.01) {
                place.last_requested = place.last_requested.max(slot.last_requested);
                *slot = place;
            }
        }
    }

    async fn refresh_station(&self, place: &mut Place, now: DateTime<Utc>) {
        let fresh = place.station_checked.is_some_and(|t| now - t < TimeDelta::days(7));
        if fresh {
            return;
        }
        match self.nearest_station(place.point).await {
            Ok(station) => {
                if station.as_ref().map(|s| &s.id) != place.station.as_ref().map(|s| &s.id) {
                    // A different station: old scores and predictions don't carry over.
                    place.scores.clear();
                    place.pending.clear();
                }
                place.station = station;
                place.station_checked = Some(now);
            }
            Err(e) => tracing_warn(&format!("station lookup: {e}")),
        }
    }

    async fn predict(&self, place: &mut Place, station: &Station, sources: &[Arc<dyn WeatherProvider>], now: DateTime<Utc>) {
        let at = GeoPoint::new(station.lat, station.lon);
        let range = ForecastRange { hours: 72, days: 3 };
        let hour = now.duration_trunc(TimeDelta::hours(1)).unwrap_or(now);
        let forecasts = join_all(sources.iter().map(|p| p.forecast(at, range))).await;
        for (p, f) in sources.iter().zip(forecasts) {
            let Ok(f) = f else { continue };
            if f.source.provider_id != p.id() {
                continue;
            }
            for lead in LEADS {
                let target = hour + TimeDelta::hours(lead);
                let known = place.pending.iter().any(|x| x.source == p.id() && x.target == target && x.lead_h == lead);
                if known {
                    continue;
                }
                if let Some(h) = f.hourly.iter().find(|h| h.time.with_timezone(&Utc) == target) {
                    place.pending.push(Prediction {
                        source: p.id().to_owned(),
                        target,
                        lead_h: lead,
                        temp_c: h.temperature_c,
                    });
                }
            }
        }
    }

    /// The nearest station with METAR reports, if one is within reach.
    async fn nearest_station(&self, point: GeoPoint) -> Result<Option<Station>, ProviderError> {
        #[derive(Deserialize)]
        struct Info {
            #[serde(rename = "icaoId")]
            icao: Option<String>,
            site: Option<String>,
            lat: f64,
            lon: f64,
            #[serde(rename = "siteType", default)]
            site_type: Vec<String>,
        }
        let d = 0.5;
        let bbox = format!("{},{},{},{}", point.lat - d, point.lon - d, point.lat + d, point.lon + d);
        let req = self
            .client
            .get(format!("{}/stationinfo", self.base_url))
            .query(&[("bbox", bbox.as_str()), ("format", "json")]);
        let list: Vec<Info> = get_json("metar", req).await?;
        Ok(list
            .into_iter()
            .filter(|s| s.site_type.iter().any(|t| t == "METAR"))
            .filter_map(|s| {
                let distance_km = km(point, GeoPoint::new(s.lat, s.lon));
                Some(Station {
                    id: s.icao?,
                    name: s.site.unwrap_or_default(),
                    lat: s.lat,
                    lon: s.lon,
                    distance_km: (distance_km * 10.0).round() / 10.0,
                })
            })
            .filter(|s| s.distance_km <= MAX_STATION_KM)
            .min_by(|a, b| a.distance_km.total_cmp(&b.distance_km)))
    }

    /// Recent temperature readings (time, °C).
    async fn readings(&self, station: &str) -> Result<Vec<(DateTime<Utc>, f64)>, ProviderError> {
        #[derive(Deserialize)]
        struct Metar {
            #[serde(rename = "obsTime")]
            obs_time: i64,
            temp: Option<f64>,
        }
        let req = self
            .client
            .get(format!("{}/metar", self.base_url))
            .query(&[("ids", station), ("format", "json"), ("hours", "4")]);
        let list: Vec<Metar> = get_json("metar", req).await?;
        Ok(list
            .into_iter()
            .filter_map(|m| Some((DateTime::from_timestamp(m.obs_time, 0)?, m.temp?)))
            .collect())
    }
}

/// Scores the predictions whose hour has a reading, and drops ones that never will.
fn score(place: &mut Place, readings: &[(DateTime<Utc>, f64)], now: DateTime<Utc>) {
    for &(time, temp) in readings {
        // Reports come on or near the hour; anything else doesn't match a forecast hour.
        let Ok(hour) = time.duration_round(TimeDelta::hours(1)) else { continue };
        if (time - hour).num_minutes().abs() > 20 {
            continue;
        }
        let (due, rest): (Vec<_>, Vec<_>) = std::mem::take(&mut place.pending).into_iter().partition(|p| p.target == hour);
        place.pending = rest;
        for p in due {
            place.scores.entry(p.source).or_default().add(p.temp_c - temp);
        }
        place.last_reading = place.last_reading.max(Some(time));
    }
    place.pending.retain(|p| now - p.target < TimeDelta::hours(6));
}

fn tracing_warn(message: &str) {
    // weather-core has no logging dependency; stderr reaches the journal.
    eprintln!("verify: {message}");
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use chrono::TimeZone;

    use crate::types::{Capability, Condition, Forecast, HourlyPoint, SourceMeta};

    const CAPE_TOWN: GeoPoint = GeoPoint { lat: -33.93, lon: 18.42 };

    fn t(h: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 27, h, 0, 0).unwrap()
    }

    fn station() -> Station {
        Station { id: "FACT".into(), name: "Capetown Intl".into(), lat: -33.965, lon: 18.602, distance_km: 17.0 }
    }

    /// A source whose every hour is `temp`.
    struct Flat(&'static str, f64);

    #[async_trait]
    impl WeatherProvider for Flat {
        fn id(&self) -> &str {
            self.0
        }
        fn name(&self) -> &str {
            self.0
        }
        fn capabilities(&self) -> &[Capability] {
            &[Capability::Hourly]
        }
        async fn forecast(&self, _: GeoPoint, _: ForecastRange) -> Result<Forecast, ProviderError> {
            let start = Utc::now().duration_trunc(TimeDelta::hours(1)).unwrap();
            Ok(Forecast {
                timezone: "UTC".into(),
                hourly: (0..72)
                    .map(|i| HourlyPoint {
                        time: (start + TimeDelta::hours(i)).fixed_offset(),
                        temperature_c: self.1,
                        precip_prob_pct: None,
                        precip_mm: None,
                        wind_speed_ms: None,
                        condition: Condition::Clear,
                        is_day: true,
                    })
                    .collect(),
                daily: vec![],
                source: SourceMeta { provider_id: self.0.into(), fetched_at: Utc::now() },
            })
        }
    }

    fn place_with_station() -> Place {
        Place {
            point: CAPE_TOWN,
            last_requested: t(0),
            station: Some(station()),
            station_checked: Some(t(0)),
            pending: vec![],
            scores: BTreeMap::new(),
            last_reading: None,
        }
    }

    #[test]
    fn scores_predictions_against_readings() {
        let mut place = place_with_station();
        for (source, temp) in [("good", 20.5), ("warm", 23.0)] {
            place.pending.push(Prediction { source: source.into(), target: t(12), lead_h: 6, temp_c: temp });
        }
        place.pending.push(Prediction { source: "good".into(), target: t(13), lead_h: 6, temp_c: 20.0 });

        // 12:00 reading (reported at 12:00), plus one off the hour that matches nothing.
        let readings = [(t(12), 20.0), (t(12) + TimeDelta::minutes(30), 21.0)];
        score(&mut place, &readings, t(12));

        assert_eq!(place.scores["good"].samples, 1);
        assert!((place.scores["good"].mae - 0.5).abs() < 1e-9);
        assert!((place.scores["warm"].bias - 3.0).abs() < 1e-9);
        assert_eq!(place.pending.len(), 1, "13:00 still waits for its reading");
        assert_eq!(place.last_reading, Some(t(12)), "only on-the-hour readings count");

        // Never read: dropped six hours later.
        score(&mut place, &[], t(19) + TimeDelta::minutes(1));
        assert!(place.pending.is_empty());
    }

    #[test]
    fn weights_favour_the_accurate_source_once_measured() {
        let v = Verifier::new(reqwest::Client::new());
        let mut place = place_with_station();
        let mut good = Score::default();
        let mut poor = Score::default();
        for _ in 0..MIN_SAMPLES {
            good.add(0.8);
            poor.add(-2.5);
        }
        place.scores.insert("gfs".into(), good);
        place.scores.insert("ecmwf".into(), poor);
        v.state.lock().unwrap().places.push(place);

        // A point 5 km away uses the same record.
        let r = v.report(GeoPoint::new(-33.97, 18.45), &["ecmwf", "gfs", "gem"]);
        assert_eq!(r.station.unwrap().id, "FACT");
        let w: BTreeMap<_, _> = r.sources.iter().map(|s| (s.id.as_str(), s)).collect();
        assert!(w["gfs"].measured && w["ecmwf"].measured && !w["gem"].measured);
        assert!(w["gfs"].weight > w["gem"].weight && w["gem"].weight > w["ecmwf"].weight);
        assert_eq!(w["ecmwf"].bias, Some(-2.5));
        let total: f64 = r.sources.iter().map(|s| s.weight).sum();
        assert!((total - 1.0).abs() < 1e-9);
    }

    #[test]
    fn without_a_record_the_starting_estimates_apply() {
        let v = Verifier::new(reqwest::Client::new());
        let r = v.report(CAPE_TOWN, &["ecmwf", "gem"]);
        assert!(r.station.is_none());
        assert!(r.sources[0].weight > r.sources[1].weight);
        assert!(r.sources.iter().all(|s| !s.measured && s.bias.is_none()));
    }

    #[test]
    fn tracks_places_once_and_survives_a_restart() {
        let v = Verifier::new(reqwest::Client::new());
        v.track(CAPE_TOWN, t(0));
        v.track(GeoPoint::new(-33.931, 18.421), t(1)); // same place
        v.track(GeoPoint::new(-26.2, 28.05), t(1)); // Johannesburg
        v.track(GeoPoint::new(f64::NAN, 0.0), t(1));
        assert_eq!(v.state.lock().unwrap().places.len(), 2);

        let restored = Verifier::new(reqwest::Client::new());
        restored.load(&v.to_json()).unwrap();
        let s = restored.state.lock().unwrap();
        assert_eq!(s.places.len(), 2);
        assert_eq!(s.places[0].last_requested, t(1));
    }

    #[tokio::test]
    async fn records_a_prediction_per_source_and_lead() {
        let v = Verifier::new(reqwest::Client::new());
        let mut place = place_with_station();
        let sources: Vec<Arc<dyn WeatherProvider>> = vec![Arc::new(Flat("a", 20.0)), Arc::new(Flat("b", 21.0))];
        let now = Utc::now();
        v.predict(&mut place, &station(), &sources, now).await;
        assert_eq!(place.pending.len(), 2 * LEADS.len());
        // Again within the hour: nothing new.
        v.predict(&mut place, &station(), &sources, now).await;
        assert_eq!(place.pending.len(), 2 * LEADS.len());
    }

    #[tokio::test]
    #[ignore = "hits the network"]
    async fn live_station_and_readings() {
        let v = Verifier::new(crate::http::default_client());
        let s = v.nearest_station(CAPE_TOWN).await.unwrap().unwrap();
        assert_eq!(s.id, "FACT");
        assert!((10.0..25.0).contains(&s.distance_km), "{s:?}");
        let r = v.readings(&s.id).await.unwrap();
        assert!(!r.is_empty());
    }
}
