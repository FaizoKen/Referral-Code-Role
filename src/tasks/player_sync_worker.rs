use std::sync::Arc;

use tokio::sync::mpsc;

use crate::services::sync::{self, PlayerSyncEvent};
use crate::AppState;

pub async fn run(mut rx: mpsc::Receiver<PlayerSyncEvent>, state: Arc<AppState>) {
    tracing::info!("Player sync worker started");

    while let Some(event) = rx.recv().await {
        tracing::debug!(discord_id = event.discord_id, "Syncing roles for user");
        if let Err(e) = sync::sync_for_player(&event.discord_id, &state).await {
            tracing::error!(discord_id = event.discord_id, "Player sync failed: {e}");
        }
    }

    tracing::warn!("Player sync worker channel closed");
}
