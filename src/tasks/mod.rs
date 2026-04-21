use std::sync::Arc;
use std::time::Duration;

use crate::AppState;

pub mod config_sync_worker;
pub mod pending_poller;
pub mod player_sync_worker;
pub mod role_expiry_worker;

pub async fn cleanup_expired(state: Arc<AppState>) {
    tracing::info!("Cleanup worker started");
    let mut interval = tokio::time::interval(Duration::from_secs(300));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        interval.tick().await;
        if let Err(e) = sqlx::query("DELETE FROM oauth_states WHERE expires_at < now()")
            .execute(&state.pool)
            .await
        {
            tracing::error!("oauth_states cleanup failed: {e}");
        }
        if let Err(e) = sqlx::query(
            "DELETE FROM redemption_attempts WHERE attempted_at < now() - interval '7 days'",
        )
        .execute(&state.pool)
        .await
        {
            tracing::error!("redemption_attempts cleanup failed: {e}");
        }
    }
}
