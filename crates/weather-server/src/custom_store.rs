//! Custom providers configured in the web app.
//!
//! The browser's SQLite database is the source of truth and pushes the full
//! list here whenever it changes. The server keeps a copy on disk so the MCP
//! tools can use custom providers while no browser tab is open.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use weather_core::{
    providers::{custom::ID_PREFIX, CustomProvider, CustomProviderConfig},
    ProviderInfo,
};

use crate::state::AppState;

const FILE_NAME: &str = "custom-providers.json";

fn file(dir: &Path) -> PathBuf {
    dir.join(FILE_NAME)
}

/// Replaces all custom providers. Validates everything before changing anything.
pub async fn replace_all(state: &AppState, configs: Vec<CustomProviderConfig>) -> Result<Vec<ProviderInfo>, String> {
    let _guard = state.custom_lock.lock().await;

    let mut seen = std::collections::HashSet::new();
    let mut providers = Vec::with_capacity(configs.len());
    for c in &configs {
        if !seen.insert(c.slug.clone()) {
            return Err(format!("duplicate slug '{}'", c.slug));
        }
        let p = CustomProvider::new(c.clone(), state.http.clone()).map_err(|e| format!("{}: {e}", c.name))?;
        providers.push(p);
    }

    if let Some(dir) = &state.data_dir {
        save(dir, &configs).map_err(|e| {
            tracing::error!(error = %e, "could not save custom providers");
            "could not save custom providers on the server".to_string()
        })?;
    }

    let mut registry = state.registry.write().expect("registry lock poisoned");
    let stale: Vec<String> = registry
        .list()
        .into_iter()
        .map(|p| p.id)
        .filter(|id| id.starts_with(ID_PREFIX))
        .collect();
    for id in stale {
        registry.unregister(&id);
    }
    for p in providers {
        registry.register(Arc::new(p));
    }
    Ok(registry.list())
}

/// Registers providers saved by a previous run. Bad entries are skipped.
pub fn load_into(state: &AppState) {
    let Some(dir) = &state.data_dir else { return };
    let path = file(dir);
    let configs: Vec<CustomProviderConfig> = match std::fs::read(&path) {
        Ok(bytes) => match serde_json::from_slice(&bytes) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(path = %path.display(), error = %e, "ignoring unreadable custom provider file");
                return;
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
        Err(e) => {
            tracing::warn!(path = %path.display(), error = %e, "could not read custom provider file");
            return;
        }
    };
    let mut registry = state.registry.write().expect("registry lock poisoned");
    for c in configs {
        match CustomProvider::new(c.clone(), state.http.clone()) {
            Ok(p) => registry.register(Arc::new(p)),
            Err(e) => tracing::warn!(slug = %c.slug, error = %e, "skipping invalid custom provider"),
        }
    }
}

/// Writes atomically; the file can hold API keys, so it is owner-only.
fn save(dir: &Path, configs: &[CustomProviderConfig]) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let tmp = dir.join(format!("{FILE_NAME}.tmp"));
    std::fs::write(&tmp, serde_json::to_vec_pretty(configs)?)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))?;
    }
    std::fs::rename(tmp, file(dir))
}

#[cfg(test)]
mod tests {
    use super::*;
    use weather_core::{http::default_client, providers::custom::{CurrentMapping, FieldMap}, ProviderRegistry};

    fn cfg(slug: &str) -> CustomProviderConfig {
        CustomProviderConfig {
            slug: slug.into(),
            name: slug.to_uppercase(),
            url_template: "https://example.com/?lat={lat}&lon={lon}".into(),
            headers: Default::default(),
            api_key: Some("k".into()),
            mapping: CurrentMapping {
                temperature_c: Some(FieldMap { path: "t".into(), transform: None }),
                ..Default::default()
            },
            condition_map: Default::default(),
        }
    }

    #[tokio::test]
    async fn replaces_persists_and_reloads() {
        let dir = std::env::temp_dir().join(format!("weather-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let state = AppState::new(ProviderRegistry::new(), default_client(), Some(dir.clone()));

        let ids = |list: Vec<ProviderInfo>| list.into_iter().map(|p| p.id).collect::<Vec<_>>();
        assert_eq!(ids(replace_all(&state, vec![cfg("a"), cfg("b")]).await.unwrap()), ["custom:a", "custom:b"]);
        assert_eq!(ids(replace_all(&state, vec![cfg("b")]).await.unwrap()), ["custom:b"]);

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(file(&dir)).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }

        let fresh = AppState::new(ProviderRegistry::new(), default_client(), Some(dir.clone()));
        load_into(&fresh);
        assert_eq!(ids(fresh.registry.read().unwrap().list()), ["custom:b"]);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn rejects_invalid_or_duplicate_without_changes() {
        let state = AppState::new(ProviderRegistry::new(), default_client(), None);
        replace_all(&state, vec![cfg("a")]).await.unwrap();

        let mut bad = cfg("b");
        bad.url_template = "nope".into();
        assert!(replace_all(&state, vec![cfg("c"), bad]).await.is_err());
        assert!(replace_all(&state, vec![cfg("d"), cfg("d")]).await.unwrap_err().contains("duplicate"));
        assert_eq!(state.registry.read().unwrap().list()[0].id, "custom:a");
    }
}
