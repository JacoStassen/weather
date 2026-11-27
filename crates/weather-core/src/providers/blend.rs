//! Our own forecast: the other sources combined, each weighted by how accurate
//! it has been near the place (see [`crate::verify`]).

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use futures::future::join_all;

use crate::error::ProviderError;
use crate::provider::WeatherProvider;
use crate::types::*;
use crate::verify::Verifier;

pub const ID: &str = "blend";

const CAPABILITIES: &[Capability] = &[Capability::Current, Capability::Hourly, Capability::Daily];

pub struct BlendProvider {
    sources: Vec<Arc<dyn WeatherProvider>>,
    verifier: Arc<Verifier>,
}

impl BlendProvider {
    /// `sources` should be independent forecasts (not Open-Meteo's own blend,
    /// which would count ECMWF twice).
    pub fn new(sources: Vec<Arc<dyn WeatherProvider>>, verifier: Arc<Verifier>) -> Self {
        Self { sources, verifier }
    }

    pub fn source_ids(&self) -> Vec<&str> {
        self.sources.iter().map(|s| s.id()).collect()
    }

    fn weights(&self, point: GeoPoint) -> HashMap<String, f64> {
        self.verifier
            .report(point, &self.source_ids())
            .sources
            .into_iter()
            .map(|s| (s.id, s.weight))
            .collect()
    }

    fn source(&self) -> SourceMeta {
        SourceMeta {
            provider_id: ID.to_owned(),
            fetched_at: Utc::now(),
        }
    }
}

/// Weighted mean of the values that are present.
fn mean(items: impl IntoIterator<Item = (f64, Option<f64>)>) -> Option<f64> {
    let (sum, total) = items
        .into_iter()
        .filter_map(|(w, v)| v.map(|v| (w * v, w)))
        .fold((0.0, 0.0), |(s, t), (wv, w)| (s + wv, t + w));
    (total > 0.0).then(|| sum / total)
}

/// Weighted mean of directions (degrees), taken on the circle so 350° and 10° give 0°.
fn mean_direction(items: impl IntoIterator<Item = (f64, Option<f64>)>) -> Option<f64> {
    let (x, y, total) = items
        .into_iter()
        .filter_map(|(w, d)| d.map(|d| (w, d.to_radians())))
        .fold((0.0, 0.0, 0.0), |(x, y, t), (w, r)| (x + w * r.cos(), y + w * r.sin(), t + w));
    (total > 0.0 && (x != 0.0 || y != 0.0)).then(|| y.atan2(x).to_degrees().rem_euclid(360.0))
}

/// The condition with the most weight behind it.
fn vote(items: impl IntoIterator<Item = (f64, Condition)>) -> Condition {
    let mut tally: Vec<(Condition, f64)> = Vec::new();
    for (w, c) in items {
        match tally.iter_mut().find(|(k, _)| *k == c) {
            Some((_, t)) => *t += w,
            None => tally.push((c, w)),
        }
    }
    tally
        .into_iter()
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .map_or(Condition::Unknown, |(c, _)| c)
}

#[async_trait]
impl WeatherProvider for BlendProvider {
    fn id(&self) -> &str {
        ID
    }

    fn name(&self) -> &str {
        "Our blend"
    }

    fn description(&self) -> Option<&str> {
        Some(
            "All the single-model sources and Yr combined, each counting according to how accurate its temperature \
             forecasts have been at the weather station nearest you. Starts from estimated weights and learns over \
             a few days.",
        )
    }

    fn capabilities(&self) -> &[Capability] {
        CAPABILITIES
    }

    async fn current(&self, point: GeoPoint) -> Result<CurrentConditions, ProviderError> {
        let weights = self.weights(point);
        let results = join_all(self.sources.iter().map(|s| s.current(point))).await;
        let got: Vec<(f64, CurrentConditions)> = self
            .sources
            .iter()
            .zip(results)
            .filter_map(|(s, r)| Some((weights.get(s.id()).copied().unwrap_or(0.0), r.ok()?)))
            .filter(|(w, _)| *w > 0.0)
            .collect();
        // The most trusted source supplies what can't be averaged.
        let (_, lead) = got
            .iter()
            .max_by(|a, b| a.0.total_cmp(&b.0))
            .ok_or(ProviderError::NoProvider { capability: Capability::Current })?;

        let field = |f: fn(&CurrentConditions) -> Option<f64>| mean(got.iter().map(|(w, c)| (*w, f(c))));
        Ok(CurrentConditions {
            observed_at: lead.observed_at,
            temperature_c: field(|c| Some(c.temperature_c)).unwrap_or(lead.temperature_c),
            feels_like_c: field(|c| c.feels_like_c),
            humidity_pct: field(|c| c.humidity_pct),
            wind_speed_ms: field(|c| c.wind_speed_ms),
            wind_gust_ms: field(|c| c.wind_gust_ms),
            wind_dir_deg: mean_direction(got.iter().map(|(w, c)| (*w, c.wind_dir_deg))),
            pressure_hpa: field(|c| c.pressure_hpa),
            precip_mm: field(|c| c.precip_mm),
            cloud_cover_pct: field(|c| c.cloud_cover_pct),
            uv_index: field(|c| c.uv_index),
            condition: vote(got.iter().map(|(w, c)| (*w, c.condition))),
            is_day: lead.is_day,
            source: self.source(),
        })
    }

    async fn forecast(&self, point: GeoPoint, range: ForecastRange) -> Result<Forecast, ProviderError> {
        let weights = self.weights(point);
        let results = join_all(self.sources.iter().map(|s| s.forecast(point, range))).await;
        let got: Vec<(f64, Forecast)> = self
            .sources
            .iter()
            .zip(results)
            .filter_map(|(s, r)| {
                let f = r.ok()?;
                // Skip answers another provider filled in.
                (f.source.provider_id == s.id()).then(|| (weights.get(s.id()).copied().unwrap_or(0.0), f))
            })
            .filter(|(w, _)| *w > 0.0)
            .collect();

        // The timeline follows the source reaching furthest ahead (ties: the most trusted),
        // so the blend covers as many days as any source does.
        let (_, base) = got
            .iter()
            .max_by(|a, b| (a.1.daily.len(), a.1.hourly.len()).cmp(&(b.1.daily.len(), b.1.hourly.len())).then(a.0.total_cmp(&b.0)))
            .ok_or(ProviderError::NoProvider { capability: Capability::Daily })?;

        let hourly = base
            .hourly
            .iter()
            .map(|h| {
                let at: Vec<(f64, &HourlyPoint)> = got
                    .iter()
                    .filter_map(|(w, f)| Some((*w, f.hourly.iter().find(|x| x.time == h.time)?)))
                    .collect();
                HourlyPoint {
                    time: h.time,
                    temperature_c: mean(at.iter().map(|(w, x)| (*w, Some(x.temperature_c)))).unwrap_or(h.temperature_c),
                    precip_prob_pct: mean(at.iter().map(|(w, x)| (*w, x.precip_prob_pct))),
                    precip_mm: mean(at.iter().map(|(w, x)| (*w, x.precip_mm))),
                    wind_speed_ms: mean(at.iter().map(|(w, x)| (*w, x.wind_speed_ms))),
                    condition: vote(at.iter().map(|(w, x)| (*w, x.condition))),
                    is_day: h.is_day,
                }
            })
            .collect();

        let daily = base
            .daily
            .iter()
            .map(|d| {
                let on: Vec<(f64, &DailyPoint)> = got
                    .iter()
                    .filter_map(|(w, f)| Some((*w, f.daily.iter().find(|x| x.date == d.date)?)))
                    .collect();
                DailyPoint {
                    date: d.date,
                    min_c: mean(on.iter().map(|(w, x)| (*w, Some(x.min_c)))).unwrap_or(d.min_c),
                    max_c: mean(on.iter().map(|(w, x)| (*w, Some(x.max_c)))).unwrap_or(d.max_c),
                    precip_mm: mean(on.iter().map(|(w, x)| (*w, x.precip_mm))).map(|mm| (mm * 10.0).round() / 10.0),
                    precip_prob_pct: mean(on.iter().map(|(w, x)| (*w, x.precip_prob_pct))),
                    sunrise: d.sunrise,
                    sunset: d.sunset,
                    condition: vote(on.iter().map(|(w, x)| (*w, x.condition))),
                }
            })
            .collect();

        Ok(Forecast {
            timezone: base.timezone.clone(),
            hourly,
            daily,
            source: self.source(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{FixedOffset, NaiveDate, TimeZone};

    struct Fixed {
        id: &'static str,
        temp: f64,
        condition: Condition,
        days: usize,
        fail: bool,
    }

    #[async_trait]
    impl WeatherProvider for Fixed {
        fn id(&self) -> &str {
            self.id
        }
        fn name(&self) -> &str {
            self.id
        }
        fn capabilities(&self) -> &[Capability] {
            CAPABILITIES
        }
        async fn current(&self, _: GeoPoint) -> Result<CurrentConditions, ProviderError> {
            if self.fail {
                return Err(ProviderError::NoProvider { capability: Capability::Current });
            }
            Ok(CurrentConditions {
                observed_at: FixedOffset::east_opt(7200).unwrap().with_ymd_and_hms(2026, 11, 27, 9, 0, 0).unwrap(),
                temperature_c: self.temp,
                feels_like_c: None,
                humidity_pct: Some(50.0),
                wind_speed_ms: Some(5.0),
                wind_gust_ms: None,
                wind_dir_deg: Some(if self.temp > 20.0 { 350.0 } else { 10.0 }),
                pressure_hpa: None,
                precip_mm: None,
                cloud_cover_pct: None,
                uv_index: None,
                condition: self.condition,
                is_day: true,
                source: SourceMeta { provider_id: self.id.into(), fetched_at: Utc::now() },
            })
        }
        async fn forecast(&self, _: GeoPoint, _: ForecastRange) -> Result<Forecast, ProviderError> {
            if self.fail {
                return Err(ProviderError::NoProvider { capability: Capability::Daily });
            }
            let day = |i: u64| NaiveDate::from_ymd_opt(2026, 11, 27).unwrap() + chrono::Days::new(i);
            Ok(Forecast {
                timezone: "Africa/Johannesburg".into(),
                hourly: vec![],
                daily: (0..self.days as u64)
                    .map(|i| DailyPoint {
                        date: day(i),
                        min_c: self.temp - 8.0,
                        max_c: self.temp,
                        precip_mm: Some(1.0),
                        precip_prob_pct: None,
                        sunrise: None,
                        sunset: None,
                        condition: self.condition,
                    })
                    .collect(),
                source: SourceMeta { provider_id: self.id.into(), fetched_at: Utc::now() },
            })
        }
    }

    fn blend(sources: Vec<Fixed>) -> BlendProvider {
        BlendProvider::new(
            sources.into_iter().map(|s| Arc::new(s) as Arc<dyn WeatherProvider>).collect(),
            Arc::new(Verifier::new(reqwest::Client::new())),
        )
    }

    const HERE: GeoPoint = GeoPoint { lat: -33.93, lon: 18.42 };

    #[tokio::test]
    async fn current_leans_towards_the_trusted_source() {
        // Starting estimates: ECMWF (1.5 °C) outweighs GEM (2.0 °C).
        let b = blend(vec![
            Fixed { id: "ecmwf", temp: 24.0, condition: Condition::Clear, days: 7, fail: false },
            Fixed { id: "gem", temp: 18.0, condition: Condition::Cloudy, days: 7, fail: false },
            Fixed { id: "gfs", temp: 30.0, condition: Condition::Rain, days: 7, fail: true },
        ]);
        let c = b.current(HERE).await.unwrap();
        assert!(c.temperature_c > 21.0 && c.temperature_c < 24.0, "{}", c.temperature_c);
        assert_eq!(c.condition, Condition::Clear);
        assert_eq!(c.source.provider_id, ID);
        // 350° and 10°: north, not south.
        let dir = c.wind_dir_deg.unwrap();
        assert!(!(10.0..350.0).contains(&dir), "{dir}");
    }

    #[tokio::test]
    async fn forecast_covers_the_longest_source_and_averages_where_they_overlap() {
        let b = blend(vec![
            Fixed { id: "ecmwf", temp: 24.0, condition: Condition::Clear, days: 10, fail: false },
            Fixed { id: "ukmo", temp: 22.0, condition: Condition::Clear, days: 6, fail: false },
        ]);
        let f = b.forecast(HERE, ForecastRange::default()).await.unwrap();
        assert_eq!(f.daily.len(), 10);
        assert!(f.daily[0].max_c < 24.0 && f.daily[0].max_c > 22.0);
        assert_eq!(f.daily[9].max_c, 24.0, "only ECMWF reaches day 10");
        assert_eq!(f.daily[0].precip_mm, Some(1.0));
    }

    #[tokio::test]
    async fn fails_only_when_every_source_does() {
        let b = blend(vec![Fixed { id: "gfs", temp: 20.0, condition: Condition::Clear, days: 7, fail: true }]);
        assert!(b.current(HERE).await.is_err());
        assert!(b.forecast(HERE, ForecastRange::default()).await.is_err());
    }

    #[test]
    fn helpers() {
        assert_eq!(mean([(1.0, Some(10.0)), (3.0, Some(20.0)), (5.0, None)]), Some(17.5));
        assert_eq!(mean([(1.0, None)]), None);
        assert_eq!(vote([(0.3, Condition::Rain), (0.25, Condition::Clear), (0.25, Condition::Clear)]), Condition::Clear);
    }
}
