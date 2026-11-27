//! Runs the forecast check ([`weather_core::verify`]) about once an hour and
//! keeps its record in `DATA_DIR/verification.json`, so the blend's weights
//! survive restarts and updates.

use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::{DurationRound, TimeDelta, Utc};
use tokio_util::sync::CancellationToken;

use crate::state::AppState;

const FILE_NAME: &str = "verification.json";

fn file(dir: &Path) -> PathBuf {
    dir.join(FILE_NAME)
}

/// Restores the saved record, if any. A damaged file is set aside and the record starts over.
pub fn load(state: &AppState) {
    let Some(dir) = &state.data_dir else { return };
    let path = file(dir);
    let Ok(json) = std::fs::read_to_string(&path) else { return };
    if let Err(e) = state.verifier.load(&json) {
        tracing::warn!(error = %e, "unreadable {}, starting the forecast check over", path.display());
        let _ = std::fs::rename(&path, dir.join(format!("{FILE_NAME}.bad")));
    }
}

fn save(state: &AppState) {
    let Some(dir) = &state.data_dir else { return };
    let write = || -> std::io::Result<()> {
        std::fs::create_dir_all(dir)?;
        let tmp = dir.join(format!("{FILE_NAME}.tmp"));
        std::fs::write(&tmp, state.verifier.to_json())?;
        std::fs::rename(tmp, file(dir))
    };
    if let Err(e) = write() {
        tracing::warn!(error = %e, "could not save the forecast check");
    }
}

/// Checks shortly after start (so a new place starts collecting at once), then
/// five minutes past every hour, when the hourly station reports are in.
pub fn spawn(state: AppState, shutdown: CancellationToken) {
    tokio::spawn(async move {
        let mut wait = Duration::from_secs(60);
        loop {
            tokio::select! {
                _ = shutdown.cancelled() => return,
                _ = tokio::time::sleep(wait) => {}
            }
            let now = Utc::now();
            state.verifier.tick(&state.blend_sources, now).await;
            save(&state);

            let hour = now.duration_trunc(TimeDelta::hours(1)).unwrap_or(now);
            let next = hour + TimeDelta::minutes(65);
            wait = (next - Utc::now()).to_std().unwrap_or(Duration::from_secs(3600));
        }
    });
}
