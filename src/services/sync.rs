use std::collections::HashSet;

use crate::error::AppError;
use crate::models::condition::RoleConditions;
use crate::services::{auth_gateway, condition_eval};
use crate::AppState;

#[derive(Debug, Clone)]
pub struct PlayerSyncEvent {
    pub discord_id: String,
}

#[derive(Debug, Clone)]
pub struct ConfigSyncEvent {
    pub guild_id: String,
    pub role_id: String,
}

pub async fn sync_for_player(discord_id: &str, state: &AppState) -> Result<(), AppError> {
    let pool = &state.pool;
    let rl_client = &state.rl_client;

    let user_batches: HashSet<i64> = sqlx::query_scalar::<_, i64>(
        "SELECT DISTINCT batch_id FROM redemptions \
         WHERE discord_id = $1 AND (role_expires_at IS NULL OR role_expires_at > now())",
    )
    .bind(discord_id)
    .fetch_all(pool)
    .await?
    .into_iter()
    .collect();

    let existing: HashSet<(String, String)> = sqlx::query_as::<_, (String, String)>(
        "SELECT guild_id, role_id FROM role_assignments WHERE discord_id = $1",
    )
    .bind(discord_id)
    .fetch_all(pool)
    .await?
    .into_iter()
    .collect();

    // No active redemptions and no existing assignments — nothing to do.
    if user_batches.is_empty() && existing.is_empty() {
        return Ok(());
    }

    let guild_ids = auth_gateway::fetch_user_guild_ids(
        &state.http,
        &state.config.auth_gateway_url,
        &state.config.internal_api_key,
        discord_id,
    )
    .await?;

    // Clear pending flag for any redemptions whose guild the user has now joined.
    // Safe to run with an empty array: ANY('{}') matches nothing.
    let cleared = sqlx::query(
        "UPDATE redemptions SET pending = FALSE \
         WHERE discord_id = $1 AND guild_id = ANY($2) AND pending = TRUE",
    )
    .bind(discord_id)
    .bind(&guild_ids[..])
    .execute(pool)
    .await?;
    if cleared.rows_affected() > 0 {
        tracing::debug!(
            discord_id,
            count = cleared.rows_affected(),
            "Cleared pending redemptions"
        );
    }

    // Build the union of guilds we may need to touch: ones the user is currently
    // in (to add new roles) plus ones where they have stale assignments (to remove).
    let mut touch_guilds: HashSet<String> = guild_ids.iter().cloned().collect();
    for (gid, _) in &existing {
        touch_guilds.insert(gid.clone());
    }
    if touch_guilds.is_empty() {
        return Ok(());
    }
    let touch_vec: Vec<String> = touch_guilds.into_iter().collect();

    let role_links = sqlx::query_as::<
        _,
        (String, String, String, sqlx::types::Json<RoleConditions>),
    >(
        "SELECT guild_id, role_id, api_token, conditions FROM role_links WHERE guild_id = ANY($1)",
    )
    .bind(&touch_vec[..])
    .fetch_all(pool)
    .await?;

    for (guild_id, role_id, api_token, conditions) in &role_links {
        let qualifies = condition_eval::qualifies(&conditions.0, &user_batches);
        let currently = existing.contains(&(guild_id.clone(), role_id.clone()));
        match (qualifies, currently) {
            (true, false) => {
                match rl_client.add_user(guild_id, role_id, discord_id, api_token).await {
                    Err(AppError::RoleLinkNotFound) => {
                        delete_orphan_role_link(guild_id, role_id, pool).await;
                        continue;
                    }
                    Err(AppError::UserLimitReached { limit }) => {
                        tracing::warn!(guild_id, role_id, discord_id, limit, "User limit reached");
                        continue;
                    }
                    Err(e) => {
                        tracing::error!(guild_id, role_id, discord_id, "add_user failed: {e}");
                        continue;
                    }
                    Ok(_) => {}
                }
                if let Err(e) = sqlx::query(
                    "INSERT INTO role_assignments (guild_id, role_id, discord_id) \
                     VALUES ($1,$2,$3) ON CONFLICT DO NOTHING",
                )
                .bind(guild_id)
                .bind(role_id)
                .bind(discord_id)
                .execute(pool)
                .await
                {
                    tracing::error!(guild_id, role_id, discord_id, "insert assignment: {e}");
                }
            }
            (false, true) => {
                match rl_client
                    .remove_user(guild_id, role_id, discord_id, api_token)
                    .await
                {
                    Err(AppError::RoleLinkNotFound) => {
                        delete_orphan_role_link(guild_id, role_id, pool).await;
                        continue;
                    }
                    Err(e) => {
                        tracing::error!(guild_id, role_id, discord_id, "remove_user failed: {e}");
                        continue;
                    }
                    Ok(_) => {}
                }
                if let Err(e) = sqlx::query(
                    "DELETE FROM role_assignments \
                     WHERE guild_id = $1 AND role_id = $2 AND discord_id = $3",
                )
                .bind(guild_id)
                .bind(role_id)
                .bind(discord_id)
                .execute(pool)
                .await
                {
                    tracing::error!(guild_id, role_id, discord_id, "delete assignment: {e}");
                }
            }
            _ => {}
        }
    }

    Ok(())
}

pub async fn sync_for_role_link(
    guild_id: &str,
    role_id: &str,
    state: &AppState,
) -> Result<(), AppError> {
    let pool = &state.pool;
    let rl_client = &state.rl_client;

    let link = sqlx::query_as::<_, (String, sqlx::types::Json<RoleConditions>)>(
        "SELECT api_token, conditions FROM role_links WHERE guild_id = $1 AND role_id = $2",
    )
    .bind(guild_id)
    .bind(role_id)
    .fetch_optional(pool)
    .await?;

    let Some((api_token, conditions)) = link else {
        return Ok(());
    };

    if conditions.0.approved_batch_ids.is_empty() {
        match rl_client
            .replace_users_scalable(guild_id, role_id, &[], &api_token)
            .await
        {
            Ok(_) => {}
            Err(AppError::RoleLinkNotFound) => {
                delete_orphan_role_link(guild_id, role_id, pool).await;
                return Ok(());
            }
            Err(e) => return Err(e),
        }
        sqlx::query("DELETE FROM role_assignments WHERE guild_id = $1 AND role_id = $2")
            .bind(guild_id)
            .bind(role_id)
            .execute(pool)
            .await?;
        return Ok(());
    }

    // Fetch qualifying redemption discord_ids straight from the DB — this is
    // already scoped by guild_id and batch filter, so no in-SQL intersection
    // with the guild member list (which blew up past ~30k array params).
    let qualifying_all: Vec<String> = sqlx::query_scalar::<_, String>(
        "SELECT DISTINCT discord_id FROM redemptions \
         WHERE batch_id = ANY($1) AND guild_id = $2 \
           AND (role_expires_at IS NULL OR role_expires_at > now())",
    )
    .bind(&conditions.0.approved_batch_ids[..])
    .bind(guild_id)
    .fetch_all(pool)
    .await?;

    // Intersect with current guild members in-memory so users who have left
    // the guild aren't pushed to Role Link (they'd count against the plan).
    // If the gateway call fails, fall back to the unfiltered list — the bot
    // skips non-members during Discord sync anyway.
    let qualifying: Vec<String> = match auth_gateway::fetch_guild_member_ids(
        &state.http,
        &state.config.auth_gateway_url,
        &state.config.internal_api_key,
        guild_id,
    )
    .await
    {
        Ok(member_ids) => {
            let member_set: HashSet<String> = member_ids.into_iter().collect();
            qualifying_all
                .into_iter()
                .filter(|id| member_set.contains(id))
                .collect()
        }
        Err(e) => {
            tracing::warn!(
                guild_id,
                role_id,
                "Auth gateway member fetch failed, syncing unfiltered qualifying set: {e}"
            );
            qualifying_all
        }
    };

    tracing::info!(
        guild_id,
        role_id,
        qualifying = qualifying.len(),
        "Pushing full user list to Role Link"
    );

    match rl_client
        .replace_users_scalable(guild_id, role_id, &qualifying, &api_token)
        .await
    {
        Ok(_) => {}
        Err(AppError::RoleLinkNotFound) => {
            delete_orphan_role_link(guild_id, role_id, pool).await;
            return Ok(());
        }
        Err(e) => return Err(e),
    }

    // Rebuild the local mirror. For ≤100k users this is one UNNEST; for
    // larger lists we chunk inserts to keep bind-param sizes reasonable.
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM role_assignments WHERE guild_id = $1 AND role_id = $2")
        .bind(guild_id)
        .bind(role_id)
        .execute(&mut *tx)
        .await?;

    const ASSIGNMENT_INSERT_CHUNK: usize = 50_000;
    for chunk in qualifying.chunks(ASSIGNMENT_INSERT_CHUNK) {
        sqlx::query(
            "INSERT INTO role_assignments (guild_id, role_id, discord_id) \
             SELECT $1, $2, UNNEST($3::text[])",
        )
        .bind(guild_id)
        .bind(role_id)
        .bind(chunk)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;

    Ok(())
}

/// Delete a role_link the RoleLogic API reports as gone (403 Invalid or
/// revoked token). CASCADE clears role_assignments. Best-effort: logs DB
/// failures, never propagates them — sync workers must not stop syncing
/// other links over a cleanup hiccup.
async fn delete_orphan_role_link(guild_id: &str, role_id: &str, pool: &sqlx::PgPool) {
    tracing::warn!(
        guild_id,
        role_id,
        "Role link not found on RoleLogic; removing orphaned local row"
    );
    if let Err(e) = sqlx::query("DELETE FROM role_links WHERE guild_id = $1 AND role_id = $2")
        .bind(guild_id)
        .bind(role_id)
        .execute(pool)
        .await
    {
        tracing::error!(guild_id, role_id, "Failed to delete orphan role_link: {e}");
    }
}
