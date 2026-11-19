//! User-defined provider: any JSON HTTP API, described by a URL template and a
//! mapping from response paths to [`CurrentConditions`] fields.
//!
//! Only current conditions are supported; forecasts, alerts and marine data
//! fall back to the built-in providers through the registry.

use std::collections::BTreeMap;

use async_trait::async_trait;
use chrono::{DateTime, FixedOffset, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use schemars::JsonSchema;
use ts_rs::TS;

use crate::error::ProviderError;
use crate::http::get_json;
use crate::provider::WeatherProvider;
use crate::types::*;

pub const ID_PREFIX: &str = "custom:";
const CAPABILITIES: &[Capability] = &[Capability::Current];

/// Saved in the browser's SQLite database and sent to the server on change.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CustomProviderConfig {
    /// Slug, unique among custom providers; the provider id is `custom:<slug>`.
    pub slug: String,
    pub name: String,
    /// `{lat}`, `{lon}` and `{apiKey}` are replaced (URL-encoded).
    pub url_template: String,
    /// Extra request headers; values may contain `{apiKey}`.
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    /// The key's value, resolved from the browser's key store before sending.
    #[serde(default)]
    pub api_key: Option<String>,
    pub mapping: CurrentMapping,
    /// Maps the source's condition value (text or code, case-insensitive) to
    /// a normalized condition.
    #[serde(default)]
    pub condition_map: BTreeMap<String, Condition>,
}

/// Where each field lives in the response. Only `temperature_c` is required.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CurrentMapping {
    pub temperature_c: Option<FieldMap>,
    #[serde(default)]
    pub feels_like_c: Option<FieldMap>,
    #[serde(default)]
    pub humidity_pct: Option<FieldMap>,
    #[serde(default)]
    pub wind_speed_ms: Option<FieldMap>,
    #[serde(default)]
    pub wind_gust_ms: Option<FieldMap>,
    #[serde(default)]
    pub wind_dir_deg: Option<FieldMap>,
    #[serde(default)]
    pub pressure_hpa: Option<FieldMap>,
    #[serde(default)]
    pub precip_mm: Option<FieldMap>,
    #[serde(default)]
    pub cloud_cover_pct: Option<FieldMap>,
    #[serde(default)]
    pub uv_index: Option<FieldMap>,
    /// Text or numeric code, looked up in `condition_map`.
    #[serde(default)]
    pub condition: Option<FieldMap>,
    /// Boolean or 0/1. Defaults to day.
    #[serde(default)]
    pub is_day: Option<FieldMap>,
    /// Unix seconds or an RFC 3339 string. Defaults to the fetch time.
    #[serde(default)]
    pub observed_at: Option<FieldMap>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FieldMap {
    /// Dotted path with optional indexes, e.g. `current.temp_c` or `data[0].wind.speed`.
    pub path: String,
    #[serde(default)]
    pub transform: Option<Transform>,
}

/// Unit conversions into the normalized metric units.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Transform {
    FahrenheitToCelsius,
    KelvinToCelsius,
    KmhToMs,
    MphToMs,
    KnotsToMs,
    InhgToHpa,
    InchesToMm,
    /// 0–1 fraction to percent.
    FractionToPercent,
}

impl Transform {
    pub fn apply(self, v: f64) -> f64 {
        match self {
            Self::FahrenheitToCelsius => (v - 32.0) * 5.0 / 9.0,
            Self::KelvinToCelsius => v - 273.15,
            Self::KmhToMs => v / 3.6,
            Self::MphToMs => v * 0.44704,
            Self::KnotsToMs => v * 0.514444,
            Self::InhgToHpa => v * 33.8639,
            Self::InchesToMm => v * 25.4,
            Self::FractionToPercent => v * 100.0,
        }
    }
}

impl CustomProviderConfig {
    pub fn provider_id(&self) -> String {
        format!("{ID_PREFIX}{}", self.slug)
    }

    /// Checks the config without making a request.
    pub fn validate(&self) -> Result<(), String> {
        let slug_ok = !self.slug.is_empty()
            && self.slug.len() <= 48
            && self
                .slug
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
        if !slug_ok {
            return Err("slug must be 1-48 characters of a-z, 0-9 or '-'".into());
        }
        if self.name.trim().is_empty() {
            return Err("name is required".into());
        }
        let url = self.url_template.trim();
        if !(url.starts_with("https://") || url.starts_with("http://")) {
            return Err("URL must start with http:// or https://".into());
        }
        if !url.contains("{lat}") || !url.contains("{lon}") {
            return Err("URL must contain {lat} and {lon}".into());
        }
        if self.mapping.temperature_c.is_none() {
            return Err("a path for temperature is required".into());
        }
        for (name, value) in &self.headers {
            reqwest::header::HeaderName::from_bytes(name.as_bytes())
                .map_err(|_| format!("invalid header name '{name}'"))?;
            if value.contains('\n') || value.contains('\r') {
                return Err(format!("invalid value for header '{name}'"));
            }
        }
        for (field, map) in self.mapping.fields() {
            parse_path(&map.path).map_err(|e| format!("{field}: {e}"))?;
        }
        Ok(())
    }

    fn url_for(&self, p: GeoPoint) -> String {
        let key = self.api_key.as_deref().unwrap_or("");
        self.url_template
            .trim()
            .replace("{lat}", &format!("{:.4}", p.lat))
            .replace("{lon}", &format!("{:.4}", p.lon))
            .replace("{apiKey}", &url_encode(key))
    }
}

impl CurrentMapping {
    fn fields(&self) -> impl Iterator<Item = (&'static str, &FieldMap)> {
        [
            ("temperatureC", &self.temperature_c),
            ("feelsLikeC", &self.feels_like_c),
            ("humidityPct", &self.humidity_pct),
            ("windSpeedMs", &self.wind_speed_ms),
            ("windGustMs", &self.wind_gust_ms),
            ("windDirDeg", &self.wind_dir_deg),
            ("pressureHpa", &self.pressure_hpa),
            ("precipMm", &self.precip_mm),
            ("cloudCoverPct", &self.cloud_cover_pct),
            ("uvIndex", &self.uv_index),
            ("condition", &self.condition),
            ("isDay", &self.is_day),
            ("observedAt", &self.observed_at),
        ]
        .into_iter()
        .filter_map(|(n, m)| m.as_ref().map(|m| (n, m)))
    }
}

fn url_encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

pub struct CustomProvider {
    id: String,
    config: CustomProviderConfig,
    client: reqwest::Client,
}

impl CustomProvider {
    pub fn new(config: CustomProviderConfig, client: reqwest::Client) -> Result<Self, String> {
        config.validate()?;
        Ok(Self {
            id: config.provider_id(),
            config,
            client,
        })
    }

    /// Fetches the raw response. Used by the settings screen's "Test" button
    /// so the user can see what paths are available.
    pub async fn fetch_raw(&self, point: GeoPoint) -> Result<Value, ProviderError> {
        let mut req = self.client.get(self.config.url_for(point));
        let key = self.config.api_key.as_deref().unwrap_or("");
        for (name, value) in &self.config.headers {
            req = req.header(name, value.replace("{apiKey}", key));
        }
        get_json(&self.id, req).await
    }

    pub fn map_current(&self, raw: &Value, fetched_at: DateTime<Utc>) -> Result<CurrentConditions, ProviderError> {
        let m = &self.config.mapping;
        let bad = |message: String| ProviderError::BadResponse {
            provider: self.id.clone(),
            message,
        };
        let num = |f: &Option<FieldMap>, name: &str| -> Result<Option<f64>, ProviderError> {
            let Some(f) = f else { return Ok(None) };
            match lookup(raw, &f.path).map_err(&bad)? {
                None | Some(Value::Null) => Ok(None),
                Some(v) => {
                    let n = as_f64(v).ok_or_else(|| bad(format!("{name} at '{}' is not a number: {v}", f.path)))?;
                    Ok(Some(f.transform.map_or(n, |t| t.apply(n))))
                }
            }
        };

        let temperature_c = num(&m.temperature_c, "temperature")?
            .ok_or_else(|| bad("temperature not found in response".into()))?;

        let condition = match &m.condition {
            Some(f) => match lookup(raw, &f.path).map_err(&bad)? {
                Some(v) => {
                    let key = match v {
                        Value::String(s) => s.trim().to_lowercase(),
                        other => other.to_string(),
                    };
                    self.config
                        .condition_map
                        .iter()
                        .find(|(k, _)| k.trim().to_lowercase() == key)
                        .map(|(_, c)| *c)
                        .unwrap_or(Condition::Unknown)
                }
                None => Condition::Unknown,
            },
            None => Condition::Unknown,
        };

        let is_day = match &m.is_day {
            Some(f) => match lookup(raw, &f.path).map_err(&bad)? {
                Some(Value::Bool(b)) => *b,
                Some(v) => as_f64(v) != Some(0.0),
                None => true,
            },
            None => true,
        };

        let utc = FixedOffset::east_opt(0).expect("zero offset");
        let observed_at = match &m.observed_at {
            Some(f) => match lookup(raw, &f.path).map_err(&bad)? {
                Some(Value::String(s)) => DateTime::parse_from_rfc3339(s)
                    .map_err(|_| bad(format!("observedAt '{s}' is not an RFC 3339 time")))?,
                Some(v) => as_f64(v)
                    .and_then(|n| DateTime::from_timestamp(n as i64, 0))
                    .map(|d| d.with_timezone(&utc))
                    .ok_or_else(|| bad(format!("observedAt is not a unix time: {v}")))?,
                None => fetched_at.with_timezone(&utc),
            },
            None => fetched_at.with_timezone(&utc),
        };

        Ok(CurrentConditions {
            observed_at,
            temperature_c,
            feels_like_c: num(&m.feels_like_c, "feelsLike")?,
            humidity_pct: num(&m.humidity_pct, "humidity")?,
            wind_speed_ms: num(&m.wind_speed_ms, "windSpeed")?,
            wind_gust_ms: num(&m.wind_gust_ms, "windGust")?,
            wind_dir_deg: num(&m.wind_dir_deg, "windDir")?,
            pressure_hpa: num(&m.pressure_hpa, "pressure")?,
            precip_mm: num(&m.precip_mm, "precip")?,
            cloud_cover_pct: num(&m.cloud_cover_pct, "cloudCover")?,
            uv_index: num(&m.uv_index, "uvIndex")?,
            condition,
            is_day,
            source: SourceMeta {
                provider_id: self.id.clone(),
                fetched_at,
            },
        })
    }
}

#[async_trait]
impl WeatherProvider for CustomProvider {
    fn id(&self) -> &str {
        &self.id
    }

    fn name(&self) -> &str {
        &self.config.name
    }

    fn capabilities(&self) -> &[Capability] {
        CAPABILITIES
    }

    async fn current(&self, point: GeoPoint) -> Result<CurrentConditions, ProviderError> {
        let raw = self.fetch_raw(point).await?;
        self.map_current(&raw, Utc::now())
    }
}

// ---- JSON paths ------------------------------------------------------------

#[derive(Debug, PartialEq)]
enum Segment<'a> {
    Key(&'a str),
    Index(usize),
}

/// Parses `a.b[0].c` (an optional leading `$.` is ignored).
fn parse_path(path: &str) -> Result<Vec<Segment<'_>>, String> {
    let path = path.trim();
    let path = path.strip_prefix("$.").or_else(|| path.strip_prefix('$')).unwrap_or(path);
    if path.is_empty() {
        return Err("path is empty".into());
    }
    let mut out = Vec::new();
    for part in path.split('.') {
        let (key, mut rest) = match part.find('[') {
            Some(i) => (&part[..i], &part[i..]),
            None => (part, ""),
        };
        if key.is_empty() && rest.is_empty() {
            return Err(format!("empty segment in '{path}'"));
        }
        if !key.is_empty() {
            out.push(Segment::Key(key));
        }
        while !rest.is_empty() {
            let end = rest.find(']').ok_or_else(|| format!("missing ']' in '{path}'"))?;
            let idx = rest[1..end]
                .trim()
                .parse()
                .map_err(|_| format!("bad index in '{path}'"))?;
            out.push(Segment::Index(idx));
            rest = &rest[end + 1..];
            if !rest.is_empty() && !rest.starts_with('[') {
                return Err(format!("unexpected '{rest}' in '{path}'"));
            }
        }
    }
    Ok(out)
}

/// `Ok(None)` when the path doesn't exist in this response.
fn lookup<'v>(root: &'v Value, path: &str) -> Result<Option<&'v Value>, String> {
    let mut cur = root;
    for seg in parse_path(path)? {
        let next = match seg {
            Segment::Key(k) => cur.get(k),
            Segment::Index(i) => cur.get(i),
        };
        match next {
            Some(v) => cur = v,
            None => return Ok(None),
        }
    }
    Ok(Some(cur))
}

/// Numbers, or numeric strings (some APIs quote their numbers).
fn as_f64(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fm(path: &str, transform: Option<Transform>) -> Option<FieldMap> {
        Some(FieldMap { path: path.into(), transform })
    }

    /// Shaped like a typical third-party API response, in imperial units.
    fn config() -> CustomProviderConfig {
        CustomProviderConfig {
            slug: "my-api".into(),
            name: "My API".into(),
            url_template: "https://api.example.com/v1/now?q={lat},{lon}&key={apiKey}".into(),
            headers: BTreeMap::from([("X-Api-Key".into(), "{apiKey}".into())]),
            api_key: Some("s3cr3t/+=".into()),
            mapping: CurrentMapping {
                temperature_c: fm("current.temp_f", Some(Transform::FahrenheitToCelsius)),
                wind_speed_ms: fm("current.wind.mph", Some(Transform::MphToMs)),
                humidity_pct: fm("current.humidity", None),
                cloud_cover_pct: fm("current.clouds", Some(Transform::FractionToPercent)),
                condition: fm("current.weather[0].main", None),
                is_day: fm("current.is_day", None),
                observed_at: fm("current.dt", None),
                pressure_hpa: fm("current.missing", None),
                ..Default::default()
            },
            condition_map: BTreeMap::from([("Rain".into(), Condition::Rain)]),
        }
    }

    fn raw() -> Value {
        json!({ "current": {
            "temp_f": 50.0, "wind": { "mph": 10 }, "humidity": "81", "clouds": 0.75,
            "weather": [{ "main": "RAIN" }], "is_day": 0, "dt": 1_795_000_000
        }})
    }

    #[test]
    fn maps_and_converts_fields() {
        let p = CustomProvider::new(config(), crate::http::default_client()).unwrap();
        let c = p.map_current(&raw(), Utc::now()).unwrap();
        assert!((c.temperature_c - 10.0).abs() < 1e-9);
        assert!((c.wind_speed_ms.unwrap() - 4.4704).abs() < 1e-9);
        assert_eq!(c.humidity_pct, Some(81.0));
        assert_eq!(c.cloud_cover_pct, Some(75.0));
        assert_eq!(c.condition, Condition::Rain, "condition lookup is case-insensitive");
        assert!(!c.is_day);
        assert_eq!(c.observed_at.timestamp(), 1_795_000_000);
        assert_eq!(c.pressure_hpa, None, "missing paths become null");
        assert_eq!(c.source.provider_id, "custom:my-api");
    }

    #[test]
    fn missing_temperature_is_an_error() {
        let p = CustomProvider::new(config(), crate::http::default_client()).unwrap();
        let err = p.map_current(&json!({ "current": {} }), Utc::now()).unwrap_err();
        assert_eq!(err.code(), "bad_response");
    }

    #[test]
    fn builds_url_with_encoded_key() {
        let url = config().url_for(GeoPoint::new(52.1, -4.25));
        assert_eq!(url, "https://api.example.com/v1/now?q=52.1000,-4.2500&key=s3cr3t%2F%2B%3D");
    }

    #[test]
    fn validates_config() {
        assert!(config().validate().is_ok());
        type Breaker = fn(&mut CustomProviderConfig);
        let cases: [(&str, Breaker); 5] = [
            ("slug", |c| c.slug = "Bad Slug".into()),
            ("URL", |c| c.url_template = "ftp://x/{lat}{lon}".into()),
            ("{lat}", |c| c.url_template = "https://x/".into()),
            ("temperature", |c| c.mapping.temperature_c = None),
            ("index", |c| c.mapping.uv_index = fm("a[x]", None)),
        ];
        for (expect, break_it) in cases {
            let mut c = config();
            break_it(&mut c);
            let err = c.validate().unwrap_err();
            assert!(err.contains(expect), "{expect}: {err}");
        }
    }

    #[test]
    fn parses_paths() {
        use Segment::*;
        assert_eq!(parse_path("$.a.b[0][2].c").unwrap(), vec![Key("a"), Key("b"), Index(0), Index(2), Key("c")]);
        assert_eq!(parse_path("[1].x").unwrap(), vec![Index(1), Key("x")]);
        assert!(parse_path("a..b").is_err());
        assert!(parse_path("a[1").is_err());
        assert!(parse_path("a[1]x").is_err());
    }
}

/// Result of trying a config from the settings screen. Always returned with
/// whatever got that far, so the user can fix the mapping against `raw`.
#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CustomProviderTestResult {
    #[ts(type = "unknown")]
    pub raw: Option<Value>,
    pub current: Option<CurrentConditions>,
    pub error_code: Option<String>,
    pub error: Option<String>,
}

impl CustomProvider {
    pub async fn test(&self, point: GeoPoint) -> CustomProviderTestResult {
        let fail = |raw: Option<Value>, e: ProviderError| CustomProviderTestResult {
            raw,
            current: None,
            error_code: Some(e.code().into()),
            error: Some(e.to_string()),
        };
        let raw = match self.fetch_raw(point).await {
            Ok(raw) => raw,
            Err(e) => return fail(None, e),
        };
        match self.map_current(&raw, Utc::now()) {
            Ok(current) => CustomProviderTestResult {
                raw: Some(raw),
                current: Some(current),
                error_code: None,
                error: None,
            },
            Err(e) => fail(Some(raw), e),
        }
    }
}
