//! Request logic shared by the REST API and the MCP tools.

use std::sync::Arc;

use weather_core::{
    AlertsResponse, Capability, CurrentConditions, Forecast, ForecastRange, GeoPoint,
    MarineResponse, ProviderError, WeatherProvider,
};
use weather_core::verify::BlendReport;

use crate::{auth::Access, state::AppState};

/// Picks a provider. The registry lock is released before the caller awaits.
fn resolve(
    state: &AppState,
    access: Access,
    capability: Capability,
    point: GeoPoint,
    preferred: Option<&str>,
) -> Result<Arc<dyn WeatherProvider>, ProviderError> {
    let preferred = preferred.filter(|id| !id.trim().is_empty());
    state
        .registry
        .read()
        .expect("registry lock poisoned")
        .resolve_filtered(capability, point, preferred, |id| access.may_use(id))
}

pub async fn current(
    state: &AppState,
    access: Access,
    point: GeoPoint,
    provider: Option<&str>,
) -> Result<CurrentConditions, ProviderError> {
    state.verifier.track(point, chrono::Utc::now());
    resolve(state, access, Capability::Current, point, provider)?
        .current(point)
        .await
}

pub async fn forecast(
    state: &AppState,
    access: Access,
    point: GeoPoint,
    range: ForecastRange,
    provider: Option<&str>,
) -> Result<Forecast, ProviderError> {
    state.verifier.track(point, chrono::Utc::now());
    resolve(state, access, Capability::Hourly, point, provider)?
        .forecast(point, range)
        .await
}

/// How the blend is weighted at `point`, and against which station.
pub fn blend_report(state: &AppState, point: GeoPoint) -> BlendReport {
    let ids: Vec<&str> = state.blend_sources.iter().map(|s| s.id()).collect();
    state.verifier.report(point, &ids)
}

/// Outside every alert provider's coverage this succeeds with
/// `coverage: false` instead of failing, so callers can tell "no alerts"
/// from "no alert data for this place".
pub async fn alerts(state: &AppState, access: Access, point: GeoPoint) -> Result<AlertsResponse, ProviderError> {
    let uncovered = || AlertsResponse {
        coverage: false,
        provider_id: None,
        alerts: Vec::new(),
    };
    let provider = match resolve(state, access, Capability::Alerts, point, None) {
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

pub async fn marine(state: &AppState, access: Access, point: GeoPoint) -> Result<MarineResponse, ProviderError> {
    let provider = match resolve(state, access, Capability::Marine, point, None) {
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
