use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc::error::TrySendError;

use crate::services::sync::PlayerSyncEvent;
use crate::AppState;

const POLL_INTERVAL_SECS: u64 = 60;
const BATCH_SIZE: i64 = 500;

pub async fn run(state: Arc<AppState>) {
    tracing::info!(
        interval_secs = POLL_INTERVAL_SECS,
        "Role-expiry worker started"
    );
    let mut interval = tokio::time::interval(Duration::from_secs(POLL_INTERVAL_SECS));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        interval.tick().await;
        match poll_once(&state).await {
            Ok((expired, dispatched)) => {
                if expired > 0 {
                    tracing::info!(expired, dispatched, "Role-expiry sweep complete");
                }
            }
            Err(e) => tracing::error!("Role-expiry sweep failed: {e}"),
        }
    }
}

async fn poll_once(state: &AppState) -> Result<(usize, usize), sqlx::Error> {
    // Pick up redemptions whose role has expired but hasn't been swept yet.
    let rows: Vec<(i64, String)> = sqlx::query_as(
        "SELECT id, discord_id FROM redemptions \
         WHERE role_expires_at IS NOT NULL \
           AND role_expires_at <= now() \
           AND role_revoked_at IS NULL \
         ORDER BY role_expires_at LIMIT $1",
    )
    .bind(BATCH_SIZE)
    .fetch_all(&state.pool)
    .await?;

    if rows.is_empty() {
        return Ok((0, 0));
    }

    let expired = rows.len();
    let ids: Vec<i64> = rows.iter().map(|(id, _)| *id).collect();
    let unique_users: HashSet<String> = rows.into_iter().map(|(_, did)| did).collect();

    // Mark as swept first so we don't re-fire next cycle. The PlayerSyncEvent
    // re-evaluates qualification using `role_expires_at`, which already
    // excludes these rows; `role_revoked_at` is purely a sweep marker.
    sqlx::query("UPDATE redemptions SET role_revoked_at = now() WHERE id = ANY($1)")
        .bind(&ids[..])
        .execute(&state.pool)
        .await?;

    let mut dispatched = 0usize;
    for discord_id in unique_users {
        match state
            .player_sync_tx
            .try_send(PlayerSyncEvent { discord_id })
        {
            Ok(_) => dispatched += 1,
            Err(TrySendError::Full(_)) => {
                tracing::warn!("player_sync channel full during expiry sweep");
            }
            Err(TrySendError::Closed(_)) => {
                tracing::warn!("player_sync channel closed during expiry sweep");
                break;
            }
        }
    }

    Ok((expired, dispatched))
}
