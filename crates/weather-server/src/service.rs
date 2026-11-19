//! Request logic shared by the REST API and (in Phase 4) the MCP tools.

use std::sync::Arc;

use weather_core::{
    AlertsResponse, Capability, CurrentConditions, Forecast, ForecastRange, GeoPoint,
    MarineResponse, ProviderError, WeatherProvider,
};

use crate::state::AppState;

/// Picks a provider. The registry lock is released before the caller awaits.
fn resolve(
    state: &AppState,
    capability: Capability,
    point: GeoPoint,
    preferred: Option<&str>,
) -> Result<Arc<dyn WeatherProvider>, ProviderError> {
    let preferred = preferred.filter(|id| !id.trim().is_empty());
    state
        .registry
        .read()
        .expect("registry lock poisoned")
        .resolve(capability, point, preferred)
}

pub async fn current(
    state: &AppState,
    point: GeoPoint,
    provider: Option<&str>,
) -> Result<CurrentConditions, ProviderError> {
    resolve(state, Capability::Current, point, provider)?
        .current(point)
        .await
}

pub async fn forecast(
    state: &AppState,
    point: GeoPoint,
    range: ForecastRange,
    provider: Option<&str>,
) -> Result<Forecast, ProviderError> {
    resolve(state, Capability::Hourly, point, provider)?
        .forecast(point, range)
        .await
}

/// Outside every alert provider's coverage this succeeds with
/// `coverage: false` instead of failing, so callers can tell "no alerts"
/// from "no alert data for this place".
pub async fn alerts(state: &AppState, point: GeoPoint) -> Result<AlertsResponse, ProviderError> {
    let uncovered = || AlertsResponse {
        coverage: false,
        provider_id: None,
        alerts: Vec::new(),
    };
    let provider = match resolve(state, Capability::Alerts, point, None) {
        Ok(p) => p,
        Err(ProviderError::NoProvider { .. }) => return Ok(uncovered()),
        Err(e) => return Err(e),
    };
    match provider.alerts(point).await {
        Ok(alerts) => Ok(AlertsResponse {
            coverage: true,
            provider_id: Some(provider.id().to_owned()),
            alerts,
        }),
        Err(ProviderError::UnsupportedLocation { .. }) => Ok(uncovered()),
        Err(e) => Err(e),
    }
}

pub async fn marine(state: &AppState, point: GeoPoint) -> Result<MarineResponse, ProviderError> {
    let provider = match resolve(state, Capability::Marine, point, None) {
        Ok(p) => p,
        Err(ProviderError::NoProvider { .. }) => {
            return Ok(MarineResponse {
                coastal: false,
                report: None,
            })
        }
        Err(e) => return Err(e),
    };
    let report = provider.marine(point).await?;
    Ok(MarineResponse {
        coastal: report.is_some(),
        report,
    })
}
