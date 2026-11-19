//! Place search via the Open-Meteo geocoding API (no key needed).

use serde::{Deserialize, Serialize};
use schemars::JsonSchema;
use ts_rs::TS;

use crate::error::ProviderError;
use crate::http::get_json;

pub const GEOCODING_URL: &str = "https://geocoding-api.open-meteo.com/v1/search";
const PROVIDER: &str = "open-meteo-geocoding";

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Place {
    pub name: String,
    /// State/province, when known.
    pub region: Option<String>,
    pub country: Option<String>,
    pub country_code: Option<String>,
    pub lat: f64,
    pub lon: f64,
    pub timezone: Option<String>,
}

#[derive(Deserialize)]
struct Response {
    #[serde(default)]
    results: Vec<Hit>,
}

#[derive(Deserialize)]
struct Hit {
    name: String,
    latitude: f64,
    longitude: f64,
    admin1: Option<String>,
    country: Option<String>,
    country_code: Option<String>,
    timezone: Option<String>,
}

pub async fn search(client: &reqwest::Client, query: &str, limit: u8) -> Result<Vec<Place>, ProviderError> {
    let query = query.trim();
    // The API needs at least 2 characters.
    if query.chars().count() < 2 {
        return Ok(Vec::new());
    }
    let req = client.get(GEOCODING_URL).query(&[
        ("name", query),
        ("count", &limit.clamp(1, 20).to_string()),
        ("language", "en"),
        ("format", "json"),
    ]);
    let res: Response = get_json(PROVIDER, req).await?;
    Ok(res
        .results
        .into_iter()
        .map(|h| Place {
            name: h.name,
            region: h.admin1,
            country: h.country,
            country_code: h.country_code,
            lat: h.latitude,
            lon: h.longitude,
            timezone: h.timezone,
        })
        .collect())
}

pub const REVERSE_URL: &str = "https://nominatim.openstreetmap.org/reverse";
const REVERSE_PROVIDER: &str = "openstreetmap-nominatim";

#[derive(Deserialize)]
struct ReverseResponse {
    name: Option<String>,
    #[serde(default)]
    address: ReverseAddress,
}

#[derive(Deserialize, Default)]
struct ReverseAddress {
    city: Option<String>,
    town: Option<String>,
    village: Option<String>,
    municipality: Option<String>,
    county: Option<String>,
    state: Option<String>,
    country: Option<String>,
    country_code: Option<String>,
}

/// Names the place at a point, via OpenStreetMap Nominatim.
///
/// The point is rounded to 0.01° (~1 km) first, so the exact position never
/// leaves this machine. Nominatim asks for at most one request per second;
/// callers should cache (the server does, per rounded point).
pub async fn reverse(client: &reqwest::Client, lat: f64, lon: f64) -> Result<Option<Place>, ProviderError> {
    let (lat, lon) = ((lat * 100.0).round() / 100.0, (lon * 100.0).round() / 100.0);
    let req = client.get(REVERSE_URL).query(&[
        ("format", "jsonv2"),
        ("lat", &format!("{lat:.2}")),
        ("lon", &format!("{lon:.2}")),
        ("zoom", "10"),
        ("accept-language", "en"),
    ]);
    let res: ReverseResponse = get_json(REVERSE_PROVIDER, req).await?;
    Ok(place_from_reverse(res, lat, lon))
}

fn place_from_reverse(res: ReverseResponse, lat: f64, lon: f64) -> Option<Place> {
    let a = res.address;
    let name = a
        .city
        .or(a.town)
        .or(a.village)
        .or(a.municipality)
        .or(res.name.filter(|n| !n.is_empty()))
        .or(a.county)?;
    Some(Place {
        name,
        region: a.state,
        country: a.country,
        country_code: a.country_code.map(|c| c.to_uppercase()),
        // Keep the caller's coordinates: the result's are the place's centre.
        lat,
        lon,
        timezone: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_the_most_specific_settlement_name() {
        let res: ReverseResponse = serde_json::from_value(serde_json::json!({
            "name": "The Hague",
            "address": { "city": "The Hague", "state": "South Holland", "country": "Netherlands", "country_code": "nl" }
        }))
        .unwrap();
        let p = place_from_reverse(res, 52.11, 4.28).unwrap();
        assert_eq!((p.name.as_str(), p.country_code.as_deref(), p.lat), ("The Hague", Some("NL"), 52.11));

        let rural: ReverseResponse = serde_json::from_value(serde_json::json!({
            "name": "", "address": { "county": "Mariposa County", "state": "California" }
        }))
        .unwrap();
        assert_eq!(place_from_reverse(rural, 37.5, -120.0).unwrap().name, "Mariposa County");

        let sea: ReverseResponse = serde_json::from_value(serde_json::json!({ "error": "Unable to geocode" })).unwrap();
        assert!(place_from_reverse(sea, 52.0, 3.0).is_none());
    }
}
