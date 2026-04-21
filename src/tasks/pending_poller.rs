use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc::error::TrySendError;

use crate::services::sync::PlayerSyncEvent;
use crate::AppState;

const POLL_INTERVAL_SECS: u64 = 120;
const BATCH_SIZE: i64 = 500;

pub async fn run(state: Arc<AppState>) {
    tracing::info!(
        interval_secs = POLL_INTERVAL_SECS,
        "Pending-redemption poller started"
    );
    let mut interval = tokio::time::interval(Duration::from_secs(POLL_INTERVAL_SECS));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        interval.tick().await;
        match poll_once(&state).await {
            Ok((dispatched, dropped)) => {
                if dispatched > 0 || dropped > 0 {
                    tracing::info!(
                        dispatched,
                        dropped,
                        "Pending-redemption poll cycle complete"
                    );
                }
            }
            Err(e) => tracing::error!("Pending-redemption poll failed: {e}"),
        }
    }
}

async fn poll_once(state: &AppState) -> Result<(usize, usize), sqlx::Error> {
    let candidates: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT discord_id FROM redemptions \
         WHERE pending = TRUE AND redeemed_at > now() - interval '30 days' \
         ORDER BY discord_id LIMIT $1",
    )
    .bind(BATCH_SIZE)
    .fetch_all(&state.pool)
    .await?;

    let mut dispatched = 0usize;
    let mut dropped = 0usize;
    for discord_id in candidates {
        match state
            .player_sync_tx
            .try_send(PlayerSyncEvent { discord_id })
        {
            Ok(_) => dispatched += 1,
            Err(TrySendError::Full(_)) => {
                dropped += 1;
            }
            Err(TrySendError::Closed(_)) => {
                tracing::warn!("player_sync channel closed; poller stopping dispatch this cycle");
                break;
            }
        }
    }
    if dropped > 0 {
        tracing::warn!(dropped, "player_sync channel full; some events skipped");
    }
    Ok((dispatched, dropped))
}
