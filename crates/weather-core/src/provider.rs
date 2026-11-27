use async_trait::async_trait;

use crate::error::ProviderError;
use crate::types::{
    Capability, CurrentConditions, Forecast, ForecastRange, GeoPoint, MarineReport, ProviderInfo,
    WeatherAlert,
};

/// A source of weather data.
///
/// Providers differ in what they cover (Open-Meteo has no alerts, NWS only
/// covers the US), so every data method has a default that reports the
/// capability as unsupported. Implement only what the source offers and list
/// it in [`capabilities`](WeatherProvider::capabilities); the registry uses that
/// list together with [`supports`](WeatherProvider::supports) to route requests.
#[async_trait]
pub trait WeatherProvider: Send + Sync {
    /// Stable id: "open-meteo", "nws", or "custom:<slug>".
    fn id(&self) -> &str;

    /// Human-readable name for the UI.
    fn name(&self) -> &str;

    fn capabilities(&self) -> &[Capability];

    /// A sentence or two for the UI: where the data comes from and what it's good for.
    fn description(&self) -> Option<&str> {
        None
    }

    /// Whether this provider covers the given location at all.
    fn supports(&self, _point: GeoPoint) -> bool {
        true
    }

    fn can_serve(&self, capability: Capability, point: GeoPoint) -> bool {
        self.capabilities().contains(&capability) && self.supports(point)
    }

    fn info(&self) -> ProviderInfo {
        ProviderInfo {
            id: self.id().to_owned(),
            name: self.name().to_owned(),
            capabilities: self.capabilities().to_vec(),
            description: self.description().map(str::to_owned),
        }
    }

    async fn current(&self, _point: GeoPoint) -> Result<CurrentConditions, ProviderError> {
        Err(self.unsupported(Capability::Current))
    }

    async fn forecast(
        &self,
        _point: GeoPoint,
        _range: ForecastRange,
    ) -> Result<Forecast, ProviderError> {
        Err(self.unsupported(Capability::Hourly))
    }

    async fn alerts(&self, _point: GeoPoint) -> Result<Vec<WeatherAlert>, ProviderError> {
        Err(self.unsupported(Capability::Alerts))
    }

    /// `Ok(None)` means the location is not coastal (no marine data there).
    async fn marine(&self, _point: GeoPoint) -> Result<Option<MarineReport>, ProviderError> {
        Err(self.unsupported(Capability::Marine))
    }

    #[doc(hidden)]
    fn unsupported(&self, capability: Capability) -> ProviderError {
        ProviderError::UnsupportedCapability {
            provider: self.id().to_owned(),
            capability,
        }
    }
}
