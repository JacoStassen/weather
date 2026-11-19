use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use weather_core::{cache::TtlCache, geocode::Place, http::HttpClient, ProviderRegistry};

/// Shared state for the REST API and the MCP server.
#[derive(Clone)]
pub struct AppState {
    /// Behind a lock because custom providers are added and removed at runtime
    /// from the web app's settings. Clone the provider `Arc` out of the lock
    /// before awaiting on it.
    pub registry: Arc<RwLock<ProviderRegistry>>,
    pub http: HttpClient,
    /// Where server-side state lives (the custom provider file). `None` keeps
    /// everything in memory, as in tests.
    pub data_dir: Option<PathBuf>,
    /// Serializes custom-provider updates so file and registry stay in step.
    pub custom_lock: Arc<tokio::sync::Mutex<()>>,
    /// Reverse-geocoding results per ~1 km cell (Nominatim allows 1 req/s).
    pub place_names: Arc<TtlCache<(i32, i32), Option<Place>>>,
    /// Whether an `OWNER_TOKEN` is configured (reported by /api/health).
    pub owner_token_required: bool,
}

impl AppState {
    pub fn new(registry: ProviderRegistry, http: HttpClient, data_dir: Option<PathBuf>) -> Self {
        Self {
            registry: Arc::new(RwLock::new(registry)),
            http,
            data_dir,
            custom_lock: Arc::default(),
            place_names: Arc::new(TtlCache::new(std::time::Duration::from_secs(24 * 3600), 512)),
            owner_token_required: false,
        }
    }
}
