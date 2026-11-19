use std::sync::Arc;

use crate::error::ProviderError;
use crate::provider::WeatherProvider;
use crate::types::{Capability, GeoPoint, ProviderInfo};

/// Holds the available providers and decides which one serves a request.
#[derive(Clone, Default)]
pub struct ProviderRegistry {
    providers: Vec<Arc<dyn WeatherProvider>>,
    fallback: Option<String>,
}

impl ProviderRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Provider tried after the preferred one, before any other.
    pub fn with_fallback(mut self, id: impl Into<String>) -> Self {
        self.fallback = Some(id.into());
        self
    }

    /// Adds a provider, replacing any existing one with the same id.
    pub fn register(&mut self, provider: Arc<dyn WeatherProvider>) {
        match self.providers.iter().position(|p| p.id() == provider.id()) {
            Some(i) => self.providers[i] = provider,
            None => self.providers.push(provider),
        }
    }

    pub fn unregister(&mut self, id: &str) -> bool {
        let before = self.providers.len();
        self.providers.retain(|p| p.id() != id);
        self.providers.len() != before
    }

    pub fn get(&self, id: &str) -> Option<Arc<dyn WeatherProvider>> {
        self.providers.iter().find(|p| p.id() == id).cloned()
    }

    pub fn list(&self) -> Vec<ProviderInfo> {
        self.providers.iter().map(|p| p.info()).collect()
    }

    /// Picks a provider for `capability` at `point`.
    ///
    /// Order: the preferred provider, then the fallback, then the remaining
    /// providers in registration order. A preferred provider that can't serve
    /// the request is skipped rather than failing; the returned data's
    /// `SourceMeta` shows which provider actually answered. An unknown
    /// preferred id is an error.
    pub fn resolve(
        &self,
        capability: Capability,
        point: GeoPoint,
        preferred: Option<&str>,
    ) -> Result<Arc<dyn WeatherProvider>, ProviderError> {
        if !point.is_valid() {
            return Err(ProviderError::InvalidLocation {
                lat: point.lat,
                lon: point.lon,
            });
        }

        let preferred = match preferred {
            Some(id) => Some(
                self.get(id)
                    .ok_or_else(|| ProviderError::UnknownProvider(id.to_owned()))?,
            ),
            None => None,
        };
        let fallback = self.fallback.as_deref().and_then(|id| self.get(id));

        preferred
            .into_iter()
            .chain(fallback)
            .chain(self.providers.iter().cloned())
            .find(|p| p.can_serve(capability, point))
            .ok_or(ProviderError::NoProvider { capability })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;

    struct Fake {
        id: &'static str,
        caps: &'static [Capability],
        us_only: bool,
    }

    #[async_trait]
    impl WeatherProvider for Fake {
        fn id(&self) -> &str {
            self.id
        }
        fn name(&self) -> &str {
            self.id
        }
        fn capabilities(&self) -> &[Capability] {
            self.caps
        }
        fn supports(&self, p: GeoPoint) -> bool {
            !self.us_only || (24.0..=50.0).contains(&p.lat) && (-125.0..=-66.0).contains(&p.lon)
        }
    }

    fn registry() -> ProviderRegistry {
        let mut r = ProviderRegistry::new().with_fallback("open-meteo");
        r.register(Arc::new(Fake {
            id: "nws",
            caps: &[Capability::Alerts],
            us_only: true,
        }));
        r.register(Arc::new(Fake {
            id: "open-meteo",
            caps: &[Capability::Current, Capability::Hourly, Capability::Marine],
            us_only: false,
        }));
        r.register(Arc::new(Fake {
            id: "custom:mine",
            caps: &[Capability::Current],
            us_only: false,
        }));
        r
    }

    const DENVER: GeoPoint = GeoPoint { lat: 39.74, lon: -104.99 };
    const AMSTERDAM: GeoPoint = GeoPoint { lat: 52.37, lon: 4.90 };

    #[test]
    fn preferred_provider_wins_when_it_can_serve() {
        let p = registry()
            .resolve(Capability::Current, AMSTERDAM, Some("custom:mine"))
            .unwrap();
        assert_eq!(p.id(), "custom:mine");
    }

    #[test]
    fn falls_back_when_preferred_lacks_capability() {
        let p = registry()
            .resolve(Capability::Marine, AMSTERDAM, Some("custom:mine"))
            .unwrap();
        assert_eq!(p.id(), "open-meteo");
    }

    #[test]
    fn alerts_route_to_nws_inside_us_only() {
        let r = registry();
        assert_eq!(r.resolve(Capability::Alerts, DENVER, None).unwrap().id(), "nws");
        assert!(matches!(
            r.resolve(Capability::Alerts, AMSTERDAM, None),
            Err(ProviderError::NoProvider { .. })
        ));
    }

    #[test]
    fn unknown_preferred_is_an_error() {
        assert!(matches!(
            registry().resolve(Capability::Current, AMSTERDAM, Some("nope")),
            Err(ProviderError::UnknownProvider(_))
        ));
    }

    #[test]
    fn rejects_invalid_coordinates() {
        assert!(matches!(
            registry().resolve(Capability::Current, GeoPoint::new(91.0, 0.0), None),
            Err(ProviderError::InvalidLocation { .. })
        ));
    }

    #[test]
    fn register_replaces_same_id() {
        let mut r = registry();
        r.register(Arc::new(Fake {
            id: "nws",
            caps: &[],
            us_only: true,
        }));
        assert_eq!(r.list().len(), 3);
        assert!(r.get("nws").unwrap().capabilities().is_empty());
    }

    #[tokio::test]
    async fn default_methods_report_unsupported() {
        let r = registry();
        let err = r.get("nws").unwrap().current(DENVER).await.unwrap_err();
        assert_eq!(err.code(), "unsupported_capability");
    }
}
