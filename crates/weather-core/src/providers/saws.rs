//! South African Weather Service warnings, from its public CAP feed
//! (<https://caps.weathersa.co.za/Home/RssFeed>, "Public Domain").
//!
//! The feed is an RSS list of CAP 1.2 files, one per province and hazard per
//! issue, going back over a year. Each file has one `<info>` per day/level
//! group, and each of those lists municipalities with their outlines, so a
//! point can be matched exactly. The files are all `msgType` "Update" without
//! `references`: a newer file for the same province and hazard replaces an
//! older one where their periods overlap (that's how SAWS upgrades a warning,
//! e.g. Level 2 in the morning, Level 4 at noon).

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use chrono::{DateTime, FixedOffset, Utc};
use futures::{lock::Mutex as AsyncMutex, stream, StreamExt};
use serde::Deserialize;

use crate::error::ProviderError;
use crate::http::get_bytes;
use crate::provider::WeatherProvider;
use crate::types::*;

pub const ID: &str = "saws";
pub const FEED_URL: &str = "https://caps.weathersa.co.za/Home/RssFeed";

/// Warning files are only fetched from here (the feed links use http://,
/// which the site no longer answers).
const CAP_PREFIX: &str = "https://www.weathersa.co.za/Documents/Warnings/";

const CAPABILITIES: &[Capability] = &[Capability::Alerts];

/// How often the feed is re-read. Warnings are issued a few times a day.
const FEED_TTL: Duration = Duration::from_secs(5 * 60);

/// Files older than this can't hold a warning that's still running: SAWS
/// warns at most a few days ahead.
const LOOKBACK_DAYS: i64 = 7;

/// Points this close to the outline still count as South Africa (degrees,
/// ~10 km): the outline is simplified, and coastal towns must not fall off it.
const COAST_TOLERANCE_DEG: f64 = 0.1;

pub struct SawsProvider {
    client: reqwest::Client,
    feed_url: String,
    feed: Mutex<Option<(Instant, Arc<Vec<Warning>>)>>,
    /// CAP files never change once published: parsed once, kept while the
    /// feed still lists them.
    files: Mutex<HashMap<String, Arc<CapFile>>>,
    /// One refresh at a time; others wait for its result.
    refreshing: AsyncMutex<()>,
}

impl SawsProvider {
    pub fn new(client: reqwest::Client) -> Self {
        Self::with_feed_url(client, FEED_URL)
    }

    pub fn with_feed_url(client: reqwest::Client, feed_url: impl Into<String>) -> Self {
        Self {
            client,
            feed_url: feed_url.into(),
            feed: Mutex::new(None),
            files: Mutex::new(HashMap::new()),
            refreshing: AsyncMutex::new(()),
        }
    }

    fn cached(&self) -> Option<Arc<Vec<Warning>>> {
        let feed = self.feed.lock().expect("saws cache poisoned");
        feed.as_ref()
            .filter(|(at, _)| at.elapsed() < FEED_TTL)
            .map(|(_, w)| w.clone())
    }

    /// Current warnings for the whole country.
    async fn warnings(&self) -> Result<Arc<Vec<Warning>>, ProviderError> {
        if let Some(w) = self.cached() {
            return Ok(w);
        }
        let _guard = self.refreshing.lock().await;
        if let Some(w) = self.cached() {
            return Ok(w);
        }

        let now = Utc::now();
        let rss = get_bytes(ID, self.client.get(&self.feed_url)).await?;
        let urls = recent_files(&rss, now)?;

        let known = self.files.lock().expect("saws cache poisoned").clone();
        let missing: Vec<String> = urls.iter().filter(|u| !known.contains_key(*u)).cloned().collect();
        let fetched: Vec<(String, Option<CapFile>)> = stream::iter(missing)
            .map(|url| async move {
                let file = match get_bytes(ID, self.client.get(&url)).await {
                    Ok(body) => CapFile::parse(&url, &body),
                    Err(_) => None,
                };
                (url, file)
            })
            .buffer_unordered(6)
            .collect()
            .await;

        let files: Vec<Arc<CapFile>> = {
            let mut cache = self.files.lock().expect("saws cache poisoned");
            for (url, file) in fetched {
                // A file that fails is tried again on the next refresh.
                if let Some(file) = file {
                    cache.insert(url, Arc::new(file));
                }
            }
            cache.retain(|url, _| urls.contains(url));
            cache.values().cloned().collect()
        };

        let warnings = Arc::new(current_warnings(&files, now));
        *self.feed.lock().expect("saws cache poisoned") = Some((Instant::now(), warnings.clone()));
        Ok(warnings)
    }
}

#[async_trait]
impl WeatherProvider for SawsProvider {
    fn id(&self) -> &str {
        ID
    }

    fn name(&self) -> &str {
        "South African Weather Service"
    }

    fn description(&self) -> Option<&str> {
        Some(
            "Official severe weather warnings for South Africa, with impact levels. Its forecasts come from a 4.4 km \
             regional version of the UK Met Office's Unified Model, so the UK Met Office source is the closest match \
             to them.",
        )
    }

    fn capabilities(&self) -> &[Capability] {
        CAPABILITIES
    }

    fn supports(&self, p: GeoPoint) -> bool {
        in_south_africa(p)
    }

    async fn alerts(&self, point: GeoPoint) -> Result<Vec<WeatherAlert>, ProviderError> {
        if !self.supports(point) {
            return Err(ProviderError::UnsupportedLocation {
                provider: ID.to_owned(),
                lat: point.lat,
                lon: point.lon,
            });
        }
        let warnings = self.warnings().await?;
        Ok(alerts_at(&warnings, point, Utc::now()))
    }
}

// ---- feed ------------------------------------------------------------------

#[derive(Deserialize)]
struct Rss {
    channel: Channel,
}

#[derive(Deserialize)]
struct Channel {
    #[serde(default)]
    item: Vec<Item>,
}

#[derive(Deserialize)]
struct Item {
    link: String,
    #[serde(rename = "pubDate")]
    pub_date: Option<String>,
}

/// Warning file URLs published in the last few days, without duplicates.
fn recent_files(rss: &[u8], now: DateTime<Utc>) -> Result<Vec<String>, ProviderError> {
    let rss: Rss = quick_xml::de::from_reader(rss).map_err(|e| ProviderError::BadResponse {
        provider: ID.to_owned(),
        message: format!("could not parse the warnings feed: {e}"),
    })?;
    let since = now - chrono::Duration::days(LOOKBACK_DAYS);
    let mut urls: Vec<String> = Vec::new();
    for item in rss.channel.item {
        let recent = item
            .pub_date
            .as_deref()
            .and_then(|d| DateTime::parse_from_rfc2822(d.trim()).ok())
            .is_some_and(|d| d >= since);
        let url = item.link.trim().replacen("http://", "https://", 1);
        if recent && url.starts_with(CAP_PREFIX) && url.ends_with(".xml") && !urls.contains(&url) {
            urls.push(url);
        }
    }
    Ok(urls)
}

// ---- CAP -------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CapAlert {
    identifier: String,
    sent: String,
    #[serde(default)]
    status: String,
    #[serde(default)]
    msg_type: String,
    #[serde(default)]
    info: Vec<CapInfo>,
}

#[derive(Deserialize)]
struct CapInfo {
    #[serde(default)]
    language: String,
    event: String,
    #[serde(default)]
    severity: String,
    onset: Option<String>,
    effective: Option<String>,
    expires: Option<String>,
    headline: Option<String>,
    description: Option<String>,
    instruction: Option<String>,
    #[serde(default)]
    parameter: Vec<CapParameter>,
    #[serde(default)]
    area: Vec<CapArea>,
}

#[derive(Deserialize)]
struct CapParameter {
    #[serde(rename = "valueName")]
    value_name: String,
    #[serde(default)]
    value: String,
}

#[derive(Deserialize)]
struct CapArea {
    #[serde(rename = "areaDesc")]
    area_desc: String,
    #[serde(default)]
    polygon: Vec<String>,
}

/// One parsed warning file.
struct CapFile {
    sent: DateTime<FixedOffset>,
    /// Province and hazard, from the file name (`GP_CAP_SevereThunderstorms_…`).
    series: String,
    warnings: Vec<Warning>,
}

/// One `<info>` of a file: a hazard, level and period for a set of areas.
#[derive(Clone)]
struct Warning {
    id: String,
    event: String,
    severity: Severity,
    level: Option<String>,
    headline: String,
    description: String,
    instruction: Option<String>,
    onset: DateTime<FixedOffset>,
    expires: DateTime<FixedOffset>,
    areas: Vec<Area>,
}

#[derive(Clone)]
struct Area {
    name: String,
    polygons: Vec<Polygon>,
}

impl CapFile {
    fn parse(url: &str, body: &[u8]) -> Option<CapFile> {
        let cap: CapAlert = quick_xml::de::from_reader(body).ok()?;
        if cap.status != "Actual" || cap.msg_type == "Cancel" {
            return None;
        }
        let sent = DateTime::parse_from_rfc3339(cap.sent.trim()).ok()?;
        let name = url.rsplit('/').next().unwrap_or(url);
        // Drop the trailing _YYYYMMDD_HHMMSS.xml.
        let series = name.splitn(4, '_').take(3).collect::<Vec<_>>().join("_");

        let warnings = cap
            .info
            .into_iter()
            .enumerate()
            .filter(|(_, i)| i.language.is_empty() || i.language.starts_with("en"))
            .filter_map(|(n, i)| {
                let time = |s: &Option<String>| s.as_deref().and_then(|s| DateTime::parse_from_rfc3339(s.trim()).ok());
                let onset = time(&i.onset).or_else(|| time(&i.effective)).unwrap_or(sent);
                let expires = time(&i.expires)?;
                let level = i
                    .parameter
                    .iter()
                    .find(|p| p.value_name == "alertType" && !p.value.trim().is_empty())
                    .map(|p| p.value.trim().to_owned());
                let areas = i
                    .area
                    .into_iter()
                    .map(|a| Area {
                        name: a.area_desc.trim().to_owned(),
                        polygons: a.polygon.iter().filter_map(|p| Polygon::parse(p)).collect(),
                    })
                    .filter(|a| !a.polygons.is_empty())
                    .collect();
                Some(Warning {
                    id: format!("{}#{n}", cap.identifier),
                    severity: severity(&i.severity),
                    level,
                    headline: i.headline.filter(|h| !h.trim().is_empty()).unwrap_or_else(|| i.event.clone()).trim().to_owned(),
                    description: i.description.unwrap_or_default().trim().to_owned(),
                    instruction: i.instruction.map(|s| s.trim().to_owned()).filter(|s| !s.is_empty()),
                    event: i.event.trim().to_owned(),
                    onset,
                    expires,
                    areas,
                })
            })
            .collect();
        Some(CapFile { sent, series, warnings })
    }
}

fn severity(s: &str) -> Severity {
    match s.trim() {
        "Extreme" => Severity::Extreme,
        "Severe" => Severity::Severe,
        "Moderate" => Severity::Moderate,
        "Minor" => Severity::Minor,
        _ => Severity::Unknown,
    }
}

/// Warnings not yet expired, without those a newer file replaced.
fn current_warnings(files: &[Arc<CapFile>], now: DateTime<Utc>) -> Vec<Warning> {
    let mut out = Vec::new();
    for file in files {
        for w in &file.warnings {
            if w.expires <= now {
                continue;
            }
            let replaced = files.iter().any(|newer| {
                newer.series == file.series
                    && newer.sent > file.sent
                    && newer.warnings.iter().any(|n| n.onset < w.expires && w.onset < n.expires)
            });
            if !replaced {
                out.push(w.clone());
            }
        }
    }
    out
}

/// The warnings whose areas contain `p`, most severe first.
fn alerts_at(warnings: &[Warning], p: GeoPoint, now: DateTime<Utc>) -> Vec<WeatherAlert> {
    let mut alerts: Vec<WeatherAlert> = warnings
        .iter()
        .filter(|w| w.expires > now)
        .filter_map(|w| {
            let names: Vec<&str> = w
                .areas
                .iter()
                .filter(|a| a.polygons.iter().any(|poly| poly.contains(p)))
                .map(|a| a.name.as_str())
                .collect();
            if names.is_empty() {
                return None;
            }
            Some(WeatherAlert {
                id: w.id.clone(),
                event: w.event.clone(),
                severity: w.severity,
                headline: w.headline.clone(),
                description: w.description.clone(),
                instruction: w.instruction.clone(),
                effective: w.onset,
                expires: Some(w.expires),
                area_desc: names.join("; "),
                level: w.level.clone(),
                source: SourceMeta {
                    provider_id: ID.to_owned(),
                    fetched_at: now,
                },
            })
        })
        .collect();
    alerts.sort_by(|a, b| {
        b.severity
            .cmp(&a.severity)
            .then(level_number(&b.level).cmp(&level_number(&a.level)))
            .then(a.effective.cmp(&b.effective))
    });
    alerts
}

/// 4 from "Warning Level 4".
fn level_number(level: &Option<String>) -> u8 {
    level
        .as_deref()
        .and_then(|l| l.rsplit(' ').next())
        .and_then(|n| n.parse().ok())
        .unwrap_or(0)
}

// ---- geometry --------------------------------------------------------------

/// A ring of `(lat, lon)` points with its bounding box.
#[derive(Clone)]
struct Polygon {
    points: Vec<(f64, f64)>,
    bbox: (f64, f64, f64, f64),
}

impl Polygon {
    fn new(points: Vec<(f64, f64)>) -> Option<Self> {
        if points.len() < 3 {
            return None;
        }
        let mut bbox = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
        for &(lat, lon) in &points {
            bbox = (bbox.0.min(lat), bbox.1.max(lat), bbox.2.min(lon), bbox.3.max(lon));
        }
        Some(Self { points, bbox })
    }

    /// CAP: space-separated `lat,lon` pairs, first point repeated last.
    fn parse(s: &str) -> Option<Self> {
        let points = s
            .split_whitespace()
            .map(|pair| {
                let (lat, lon) = pair.split_once(',')?;
                Some((lat.parse().ok()?, lon.parse().ok()?))
            })
            .collect::<Option<Vec<(f64, f64)>>>()?;
        Self::new(points)
    }

    /// Ray casting.
    fn contains(&self, p: GeoPoint) -> bool {
        let (la0, la1, lo0, lo1) = self.bbox;
        if p.lat < la0 || p.lat > la1 || p.lon < lo0 || p.lon > lo1 {
            return false;
        }
        let mut inside = false;
        let mut j = self.points.len() - 1;
        for i in 0..self.points.len() {
            let (yi, xi) = self.points[i];
            let (yj, xj) = self.points[j];
            if (yi > p.lat) != (yj > p.lat) && p.lon < (xj - xi) * (p.lat - yi) / (yj - yi) + xi {
                inside = !inside;
            }
            j = i;
        }
        inside
    }

    /// Roughly the distance in degrees of latitude to the nearest edge.
    fn distance(&self, p: GeoPoint) -> f64 {
        let k = p.lat.to_radians().cos();
        let (px, py) = (p.lon * k, p.lat);
        let mut best = f64::MAX;
        let mut j = self.points.len() - 1;
        for i in 0..self.points.len() {
            let (ay, ax) = (self.points[j].0, self.points[j].1 * k);
            let (by, bx) = (self.points[i].0, self.points[i].1 * k);
            let (dx, dy) = (bx - ax, by - ay);
            let len2 = dx * dx + dy * dy;
            let t = if len2 == 0.0 { 0.0 } else { (((px - ax) * dx + (py - ay) * dy) / len2).clamp(0.0, 1.0) };
            let (cx, cy) = (ax + t * dx, ay + t * dy);
            best = best.min(((px - cx).powi(2) + (py - cy).powi(2)).sqrt());
            j = i;
        }
        best
    }
}

/// Mainland South Africa with Lesotho cut out (Natural Earth 1:50m, public
/// domain; Prince Edward Islands left out, as SAWS warnings don't cover them).
struct Outline {
    outer: Polygon,
    holes: Vec<Polygon>,
}

fn outline() -> &'static Outline {
    static OUTLINE: OnceLock<Outline> = OnceLock::new();
    OUTLINE.get_or_init(|| {
        #[derive(Deserialize)]
        struct Raw {
            outer: Vec<(f64, f64)>,
            holes: Vec<Vec<(f64, f64)>>,
        }
        let raw: Raw = serde_json::from_str(include_str!("za_outline.json")).expect("valid built-in outline");
        Outline {
            outer: Polygon::new(raw.outer).expect("outline has points"),
            holes: raw.holes.into_iter().filter_map(Polygon::new).collect(),
        }
    })
}

/// Inside South Africa (Lesotho excluded), allowing for the simplified coast.
fn in_south_africa(p: GeoPoint) -> bool {
    let o = outline();
    let (la0, la1, lo0, lo1) = o.outer.bbox;
    let t = COAST_TOLERANCE_DEG;
    if p.lat < la0 - t || p.lat > la1 + t || p.lon < lo0 - t || p.lon > lo1 + t {
        return false;
    }
    if o.holes.iter().any(|h| h.contains(p)) {
        return false;
    }
    o.outer.contains(p) || o.outer.distance(p) <= t
}

#[cfg(test)]
mod tests {
    use super::*;

    const GP_MORNING: &str = "https://www.weathersa.co.za/Documents/Warnings/GP_CAP_SevereThunderstorms_20261125_070805.xml";
    const GP_NOON: &str = "https://www.weathersa.co.za/Documents/Warnings/GP_CAP_SevereThunderstorms_20261125_125636.xml";
    const KZN: &str = "https://www.weathersa.co.za/Documents/Warnings/KZN_CAP_SevereThunderstorms_20261123_084602.xml";

    fn file(url: &str, xml: &str) -> Arc<CapFile> {
        Arc::new(CapFile::parse(url, xml.as_bytes()).expect("fixture parses"))
    }

    fn gauteng() -> Vec<Arc<CapFile>> {
        vec![
            file(GP_MORNING, include_str!("../../fixtures/saws_cap_gp_20261125_070805.xml")),
            file(GP_NOON, include_str!("../../fixtures/saws_cap_gp_20261125_125636.xml")),
        ]
    }

    fn at(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    const PRETORIA: GeoPoint = GeoPoint { lat: -25.7479, lon: 28.2293 };
    const JOHANNESBURG: GeoPoint = GeoPoint { lat: -26.2041, lon: 28.0473 };
    const KEMPTON_PARK: GeoPoint = GeoPoint { lat: -26.1007, lon: 28.2304 };
    const CAPE_TOWN: GeoPoint = GeoPoint { lat: -33.9249, lon: 18.4241 };

    #[test]
    fn reads_recent_unique_links_from_the_feed() {
        let urls = recent_files(include_bytes!("../../fixtures/saws_rss.xml"), at("2026-11-26T08:00:00+02:00")).unwrap();
        assert_eq!(urls.len(), 3, "{urls:?}");
        assert_eq!(urls[0], GP_NOON);
        assert!(urls.iter().all(|u| u.starts_with(CAP_PREFIX)));
        // The 2025 item is too old.
        assert!(!urls.iter().any(|u| u.contains("2025")));
    }

    #[test]
    fn parses_cap_files() {
        let f = file(GP_NOON, include_str!("../../fixtures/saws_cap_gp_20261125_125636.xml"));
        assert_eq!(f.series, "GP_CAP_SevereThunderstorms");
        assert_eq!(f.warnings.len(), 2);
        let w = &f.warnings[0];
        assert_eq!(w.event, "Severe Thunderstorms");
        assert_eq!(w.level.as_deref(), Some("Warning Level 4"));
        assert_eq!(w.severity, Severity::Moderate);
        assert!(w.instruction.as_deref().unwrap().starts_with("Seek safe shelter"));
        assert!(w.areas.iter().any(|a| a.name == "City of Tshwane / Pretoria"));
    }

    #[test]
    fn matches_the_municipality_of_the_point() {
        let warnings = current_warnings(&gauteng(), at("2026-11-25T16:00:00+02:00"));
        let now = at("2026-11-25T16:00:00+02:00");

        let pta = alerts_at(&warnings, PRETORIA, now);
        assert_eq!(pta.len(), 1);
        assert_eq!(pta[0].level.as_deref(), Some("Warning Level 4"));
        assert_eq!(pta[0].area_desc, "City of Tshwane / Pretoria");

        let jhb = alerts_at(&warnings, JOHANNESBURG, now);
        assert_eq!(jhb.len(), 1);
        assert_eq!(jhb[0].level.as_deref(), Some("Warning Level 2"));

        assert!(alerts_at(&warnings, CAPE_TOWN, now).is_empty());
    }

    #[test]
    fn a_newer_file_replaces_an_older_one() {
        // Kempton Park: Level 2 in the 07:08 file, Level 4 in the 12:56 one.
        let now = at("2026-11-25T16:00:00+02:00");
        let only_morning = current_warnings(&gauteng()[..1], now);
        assert_eq!(alerts_at(&only_morning, KEMPTON_PARK, now)[0].level.as_deref(), Some("Warning Level 2"));

        let both = current_warnings(&gauteng(), now);
        let alerts = alerts_at(&both, KEMPTON_PARK, now);
        assert_eq!(alerts.len(), 1, "the morning warning must not show as well");
        assert_eq!(alerts[0].level.as_deref(), Some("Warning Level 4"));
    }

    #[test]
    fn other_provinces_and_hazards_are_independent() {
        let mut files = gauteng();
        files.push(file(KZN, include_str!("../../fixtures/saws_cap_kzn_20261123_084602.xml")));
        let now = at("2026-11-23T14:00:00+02:00");
        let warnings = current_warnings(&files, now);
        // Pietermaritzburg: the KZN file isn't replaced by the newer Gauteng ones.
        let pmb = GeoPoint { lat: -29.6006, lon: 30.3794 };
        let alerts = alerts_at(&warnings, pmb, now);
        // Today's Level 4, then tomorrow's Level 2 (issued ahead).
        let levels: Vec<_> = alerts.iter().map(|a| a.level.as_deref().unwrap()).collect();
        assert_eq!(levels, ["Warning Level 4", "Warning Level 2"]);
        assert!(alerts[1].effective > alerts[0].effective);
    }

    #[test]
    fn expired_warnings_are_dropped() {
        let now = at("2026-11-26T01:00:00+02:00");
        assert!(current_warnings(&gauteng(), now).is_empty());
    }

    #[tokio::test]
    #[ignore]
    async fn live_saws() {
        let p = SawsProvider::new(crate::http::default_client());
        let started = Instant::now();
        let alerts = p.alerts(PRETORIA).await.unwrap();
        eprintln!("{} warning(s) for Pretoria, first fetch {:?}", alerts.len(), started.elapsed());
        let started = Instant::now();
        p.alerts(CAPE_TOWN).await.unwrap();
        assert!(started.elapsed() < Duration::from_millis(50), "second call must come from the cache");
        let err = p.alerts(GeoPoint::new(-29.3151, 27.4869)).await.unwrap_err();
        assert_eq!(err.code(), "unsupported_location");
    }

    #[test]
    fn coverage_is_south_africa_without_lesotho() {
        for (name, lat, lon) in [
            ("Cape Town", -33.9249, 18.4241),
            ("Sea Point (on the coast)", -33.9155, 18.3880),
            ("Cape Point", -34.3568, 18.4970),
            ("Durban", -29.8587, 31.0218),
            ("Gqeberha", -33.9608, 25.6022),
            ("Johannesburg", -26.2041, 28.0473),
            ("Musina", -22.3381, 30.0415),
            ("Upington", -28.4541, 21.2422),
            ("Ladybrand (next to Lesotho)", -29.1950, 27.4580),
        ] {
            assert!(in_south_africa(GeoPoint::new(lat, lon)), "{name}");
        }
        for (name, lat, lon) in [
            ("Maseru, Lesotho", -29.3151, 27.4869),
            ("Mokhotlong, Lesotho", -29.2894, 29.0646),
            ("Windhoek, Namibia", -22.5609, 17.0658),
            ("Gaborone, Botswana", -24.6282, 25.9231),
            ("Maputo, Mozambique", -25.9692, 32.5732),
            ("Amsterdam", 52.37, 4.9),
        ] {
            assert!(!in_south_africa(GeoPoint::new(lat, lon)), "{name}");
        }
    }
}
