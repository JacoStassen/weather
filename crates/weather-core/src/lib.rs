//! Provider-agnostic weather domain.
//!
//! Every data source implements [`WeatherProvider`] and returns the normalized
//! types in [`types`] (metric units: °C, m/s, hPa, mm, m). The REST API, the MCP
//! server and the web app all consume these same types; `cargo test` exports
//! them to TypeScript for the frontend.

pub mod cache;
pub mod error;
pub mod geocode;
pub mod http;
pub mod provider;
pub mod providers;
pub mod registry;
pub mod tides;
pub mod types;
pub mod verify;

pub use error::ProviderError;
pub use provider::WeatherProvider;
pub use registry::ProviderRegistry;
pub use types::*;
