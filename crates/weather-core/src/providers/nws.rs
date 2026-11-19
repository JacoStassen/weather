//! US National Weather Service (<https://www.weather.gov/documentation/services-web-api>):
//! active watches, warnings and advisories. US states and territories only.

use async_trait::async_trait;
use chrono::{DateTime, FixedOffset, Utc};
use serde::Deserialize;

use crate::error::ProviderError;
use crate::http::get_json;
use crate::provider::WeatherProvider;
use crate::types::*;

pub const ID: &str = "nws";
pub const BASE_URL: &str = "https://api.weather.gov";

const CAPABILITIES: &[Capability] = &[Capability::Alerts];

/// Rough coverage boxes `(lat_min, lat_max, lon_min, lon_max)`. Generous on
/// purpose (they include some of Canada and Mexico); the API rejects points
/// it doesn't cover, which [`NwsProvider::alerts`] reports as unsupported.
const COVERAGE: &[(f64, f64, f64, f64)] = &[
    (24.0, 50.0, -125.0, -66.0),    // contiguous US and coastal waters
    (51.0, 72.0, -180.0, -129.0),   // Alaska
    (51.0, 56.0, 172.0, 180.0),     // western Aleutians
    (18.5, 22.5, -161.0, -154.0),   // Hawaii
    (17.5, 18.8, -67.5, -64.5),     // Puerto Rico, US Virgin Islands
    (13.0, 21.0, 144.0, 146.5),     // Guam, Northern Mariana Islands
    (-14.8, -10.8, -171.5, -168.0), // American Samoa
];

pub struct NwsProvider {
    client: reqwest::Client,
    base_url: String,
}

impl NwsProvider {
    /// `client` must send a User-Agent; [`crate::http::default_client`] does.
    pub fn new(client: reqwest::Client) -> Self {
        Self::with_base_url(client, BASE_URL)
    }

    pub fn with_base_url(client: reqwest::Client, base_url: impl Into<String>) -> Self {
        Self {
            client,
            base_url: base_url.into(),
        }
    }
}

#[async_trait]
impl WeatherProvider for NwsProvider {
    fn id(&self) -> &str {
        ID
    }

    fn name(&self) -> &str {
        "National Weather Service (US)"
    }

    fn capabilities(&self) -> &[Capability] {
        CAPABILITIES
    }

    fn supports(&self, p: GeoPoint) -> bool {
        COVERAGE
            .iter()
            .any(|&(la0, la1, lo0, lo1)| (la0..=la1).contains(&p.lat) && (lo0..=lo1).contains(&p.lon))
    }

    async fn alerts(&self, point: GeoPoint) -> Result<Vec<WeatherAlert>, ProviderError> {
        let req = self
            .client
            .get(format!("{}/alerts/active", self.base_url))
            .query(&[("point", format!("{:.4},{:.4}", point.lat, point.lon))])
            .header(reqwest::header::ACCEPT, "application/geo+json");

        match get_json::<NwsCollection>(ID, req).await {
            Ok(c) => Ok(normalize(c, Utc::now())),
            // NWS answers 400 "Parameter \"point\" is invalid: out of bounds".
            Err(ProviderError::BadResponse { message, .. }) if message.contains("out of bounds") => {
                Err(ProviderError::UnsupportedLocation {
                    provider: ID.to_owned(),
                    lat: point.lat,
                    lon: point.lon,
                })
            }
            Err(e) => Err(e),
        }
    }
}

#[derive(Debug, Deserialize)]
struct NwsCollection {
    #[serde(default)]
    features: Vec<NwsFeature>,
}

#[derive(Debug, Deserialize)]
struct NwsFeature {
    properties: NwsAlert,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct NwsAlert {
    id: String,
    event: String,
    #[serde(default)]
    severity: String,
    headline: Option<String>,
    description: Option<String>,
    instruction: Option<String>,
    effective: Option<String>,
    onset: Option<String>,
    expires: Option<String>,
    ends: Option<String>,
    #[serde(default)]
    area_desc: String,
    #[serde(default)]
    status: String,
    #[serde(default)]
    message_type: String,
}

fn time(s: &Option<String>) -> Option<DateTime<FixedOffset>> {
    s.as_deref().and_then(|s| DateTime::parse_from_rfc3339(s).ok())
}

fn severity(s: &str) -> Severity {
    match s {
        "Extreme" => Severity::Extreme,
        "Severe" => Severity::Severe,
        "Moderate" => Severity::Moderate,
        "Minor" => Severity::Minor,
        _ => Severity::Unknown,
    }
}

/// Keeps real, non-cancelled alerts, most severe first.
fn normalize(c: NwsCollection, fetched_at: DateTime<Utc>) -> Vec<WeatherAlert> {
    let mut alerts: Vec<WeatherAlert> = c
        .features
        .into_iter()
        .map(|f| f.properties)
        .filter(|a| a.status == "Actual" && a.message_type != "Cancel")
        .filter_map(|a| {
            let effective = time(&a.effective).or_else(|| time(&a.onset))?;
            Some(WeatherAlert {
                severity: severity(&a.severity),
                headline: a.headline.clone().unwrap_or_else(|| a.event.clone()),
                description: a.description.clone().unwrap_or_default(),
                instruction: a.instruction.clone().filter(|s| !s.trim().is_empty()),
                effective,
                // `ends` is when the hazard ends; `expires` is when the message does.
                expires: time(&a.ends).or_else(|| time(&a.expires)),
                id: a.id,
                event: a.event,
                area_desc: a.area_desc,
                source: SourceMeta {
                    provider_id: ID.to_owned(),
                    fetched_at,
                },
            })
        })
        .collect();
    alerts.sort_by(|a, b| b.severity.cmp(&a.severity).then(b.effective.cmp(&a.effective)));
    alerts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coverage_boxes() {
        let p = NwsProvider::new(crate::http::default_client());
        for (lat, lon) in [(39.74, -104.99), (61.22, -149.9), (21.3, -157.85), (18.47, -66.1), (13.45, 144.79)] {
            assert!(p.supports(GeoPoint::new(lat, lon)), "{lat},{lon}");
        }
        for (lat, lon) in [(52.37, 4.90), (-33.92, 18.42), (35.68, 139.69)] {
            assert!(!p.supports(GeoPoint::new(lat, lon)), "{lat},{lon}");
        }
    }

    #[test]
    fn normalizes_fixture() {
        let raw: NwsCollection =
            serde_json::from_str(include_str!("../../fixtures/nws_alerts_alaska.json")).unwrap();
        let alerts = normalize(raw, Utc::now());
        assert_eq!(alerts.len(), 4);
        assert!(alerts.windows(2).all(|w| w[0].severity >= w[1].severity));
        assert!(alerts.iter().all(|a| !a.headline.is_empty() && !a.area_desc.is_empty()));
        assert!(alerts.iter().any(|a| a.instruction.is_none()));
    }

    #[test]
    fn drops_cancellations_and_tests() {
        let raw: NwsCollection = serde_json::from_value(serde_json::json!({
            "features": [
                { "properties": { "id": "a", "event": "Flood Warning", "severity": "Severe",
                  "effective": "2026-01-01T00:00:00-05:00", "status": "Actual", "messageType": "Alert" } },
                { "properties": { "id": "b", "event": "Flood Warning", "severity": "Severe",
                  "effective": "2026-01-01T00:00:00-05:00", "status": "Actual", "messageType": "Cancel" } },
                { "properties": { "id": "c", "event": "Test Message", "severity": "Unknown",
                  "effective": "2026-01-01T00:00:00-05:00", "status": "Test", "messageType": "Alert" } }
            ]
        }))
        .unwrap();
        let alerts = normalize(raw, Utc::now());
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].id, "a");
        assert_eq!(alerts[0].headline, "Flood Warning");
    }

    /// Hits the real API: `cargo test -p weather-core -- --ignored`.
    #[tokio::test]
    #[ignore]
    async fn live_nws() {
        let p = NwsProvider::new(crate::http::default_client());
        p.alerts(GeoPoint::new(39.74, -104.99)).await.unwrap();
        // Toronto is inside the generous CONUS box but outside NWS coverage.
        let err = p.alerts(GeoPoint::new(43.65, -79.38)).await.unwrap_err();
        assert_eq!(err.code(), "unsupported_location");
    }
}
