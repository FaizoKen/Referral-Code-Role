use std::sync::Arc;

use axum::extract::State;
use axum::http::HeaderMap;
use axum::Json;
use axum_extra::extract::CookieJar;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::error::AppError;
use crate::services::auth_gateway;
use crate::services::code_gen;
use crate::services::session;
use crate::services::sync::PlayerSyncEvent;
use crate::AppState;

const SESSION_COOKIE: &str = "rl_session";

fn extract_ip(headers: &HeaderMap) -> Option<String> {
    for header_name in ["cf-connecting-ip", "x-real-ip"] {
        if let Some(val) = headers.get(header_name).and_then(|v| v.to_str().ok()) {
            let ip = val.trim().to_string();
            if !ip.is_empty() {
                return Some(ip);
            }
        }
    }
    if let Some(val) = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok()) {
        if let Some(first) = val.split(',').next() {
            let ip = first.trim().to_string();
            if !ip.is_empty() {
                return Some(ip);
            }
        }
    }
    None
}

fn hash_ip(ip: &str) -> String {
    let mut h = Sha256::new();
    h.update(ip.as_bytes());
    hex::encode(h.finalize())
}

#[derive(Deserialize)]
pub struct RedeemBody {
    pub code: String,
    /// The server the redeem page was opened for (`?guild=` on a server's
    /// link). Only used to choose between servers that both have this code.
    pub guild_id: Option<String>,
}

/// A code row matching the typed key, with its group's settings.
type Candidate = (
    i64,                   // codes.id
    i64,                   // batch_id
    i32,                   // uses_count
    Option<DateTime<Utc>>, // codes.revoked_at
    String,                // batch.guild_id
    String,                // batch.kind
    Option<i32>,           // max_redemptions_per_code
    Option<i32>,           // max_redemptions_total
    Option<DateTime<Utc>>, // batch.expires_at
    Option<DateTime<Utc>>, // batch.revoked_at
    String,                // batch.name
    Option<String>,        // batch.invite_url
    Option<i32>,           // batch.role_duration_hours
);

/// Why `discord_id` can't redeem `cand` right now, or `None` if they can.
async fn unusable_reason(
    state: &AppState,
    cand: &Candidate,
    discord_id: &str,
    now: DateTime<Utc>,
) -> Result<Option<&'static str>, AppError> {
    if cand.3.is_some() {
        return Ok(Some("invalid"));
    }
    if let Some(exp) = cand.8 {
        if exp < now {
            return Ok(Some("expired"));
        }
    }
    if let Some(cap) = cand.7 {
        let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM redemptions WHERE batch_id = $1")
            .bind(cand.1)
            .fetch_one(&state.pool)
            .await?;
        if total >= cap as i64 {
            return Ok(Some("exhausted"));
        }
    }
    if cand.5 == "unique_per_code" && cand.2 >= 1 {
        return Ok(Some("already_used"));
    }
    if matches!(cand.5.as_str(), "unique_per_code" | "unique_per_user") {
        if let Some(per_cap) = cand.6 {
            if cand.2 >= per_cap {
                return Ok(Some("already_used"));
            }
        }
    }
    let already: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM redemptions WHERE code_id = $1 AND discord_id = $2)",
    )
    .bind(cand.0)
    .bind(discord_id)
    .fetch_one(&state.pool)
    .await?;
    if already {
        return Ok(Some("already_redeemed_by_you"));
    }
    Ok(None)
}

fn guild_ids(cands: &[Candidate]) -> Vec<&str> {
    cands.iter().map(|c| c.4.as_str()).collect()
}

fn one_guild(guilds: &[&str]) -> bool {
    guilds.windows(2).all(|w| w[0] == w[1])
}

/// For a code that exists in several servers, which candidates (by their
/// servers, in order) to keep: those in servers the person belongs to, since
/// that is the server they mean, or all of them when they belong to none.
fn member_guilds_mask(guilds: &[&str], member_of: &[String]) -> Vec<bool> {
    let mine: Vec<bool> = guilds
        .iter()
        .map(|g| member_of.iter().any(|m| m == g))
        .collect();
    if mine.contains(&true) {
        mine
    } else {
        vec![true; guilds.len()]
    }
}

pub async fn redeem_code(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    jar: CookieJar,
    Json(body): Json<RedeemBody>,
) -> Result<Json<Value>, AppError> {
    let cookie = jar.get(SESSION_COOKIE).ok_or(AppError::Unauthorized)?;
    let (discord_id, _display_name) =
        session::verify_session(cookie.value(), &state.config.session_secret)
            .ok_or(AppError::Unauthorized)?;

    let ip = extract_ip(&headers);
    let ip_hash = ip.as_deref().map(hash_ip);
    let user_agent = headers
        .get("user-agent")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    // Rate limit: count attempts in the last hour for this user.
    let attempts: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM redemption_attempts \
         WHERE discord_id = $1 AND attempted_at > now() - interval '1 hour'",
    )
    .bind(&discord_id)
    .fetch_one(&state.pool)
    .await?;
    if attempts >= state.config.max_redeem_attempts_per_hour {
        return Err(AppError::RateLimited);
    }
    if let Some(h) = &ip_hash {
        let ip_attempts: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM redemption_attempts \
             WHERE ip_hash = $1 AND attempted_at > now() - interval '1 hour'",
        )
        .bind(h)
        .fetch_one(&state.pool)
        .await?;
        if ip_attempts >= state.config.max_redeem_attempts_per_hour * 5 {
            return Err(AppError::RateLimited);
        }
    }

    // Hyphens, spaces and case don't matter: codes match on their key.
    let normalized = code_gen::code_key(&body.code);
    if normalized.is_empty() {
        record_attempt(&state, &discord_id, ip_hash.as_deref(), &normalized, false).await?;
        return Ok(Json(json!({"error": "invalid"})));
    }

    // Oldest first, so the same input always picks the same code.
    let mut candidates = sqlx::query_as::<_, Candidate>(
        "SELECT c.id, c.batch_id, c.uses_count, c.revoked_at, \
                b.guild_id, b.kind, b.max_redemptions_per_code, b.max_redemptions_total, \
                b.expires_at, b.revoked_at, b.name, b.invite_url, b.role_duration_hours \
         FROM codes c JOIN code_batches b ON b.id = c.batch_id \
         WHERE c.code_key = $1 AND b.revoked_at IS NULL \
         ORDER BY c.id",
    )
    .bind(&normalized)
    .fetch_all(&state.pool)
    .await?;

    if candidates.is_empty() {
        record_attempt(&state, &discord_id, ip_hash.as_deref(), &normalized, false).await?;
        return Ok(Json(json!({"error": "invalid"})));
    }

    // Codes are global, so two servers can both create the same one (say
    // PODCODE). A server's own link names it with ?guild=, and that settles
    // it: only that server's codes count, even if none can be redeemed now.
    if let Some(hint) = body.guild_id.as_deref() {
        if candidates.iter().any(|c| c.4 == hint) {
            candidates.retain(|c| c.4 == hint);
        }
    }

    // Still in several servers: the person means the one they're in, even if
    // its code is used up for them (they get that error, not another server's
    // role). Membership is fetched only here, and reused for `pending`.
    let mut member_of: Option<Vec<String>> = None;
    if !one_guild(&guild_ids(&candidates)) {
        match auth_gateway::fetch_user_guild_ids(
            &state.http,
            &state.config.auth_gateway_url,
            &state.config.internal_api_key,
            &discord_id,
        )
        .await
        {
            Ok(guilds) => {
                let mut keep = member_guilds_mask(&guild_ids(&candidates), &guilds).into_iter();
                candidates.retain(|_| keep.next().unwrap_or(false));
                member_of = Some(guilds);
            }
            Err(e) => tracing::warn!(discord_id, "auth_gateway membership check failed: {e}"),
        }
    }

    let now = Utc::now();
    let spans_guilds = !one_guild(&guild_ids(&candidates));
    let mut usable: Vec<&Candidate> = Vec::new();
    let mut last_error = "invalid";
    for cand in &candidates {
        match unusable_reason(&state, cand, &discord_id, now).await? {
            Some(reason) => last_error = reason,
            None => {
                usable.push(cand);
                // One server: the first usable code is the answer.
                if !spans_guilds {
                    break;
                }
            }
        }
    }
    if usable.is_empty() {
        record_attempt(&state, &discord_id, ip_hash.as_deref(), &normalized, false).await?;
        return Ok(Json(json!({"error": last_error})));
    }

    // Redeemable in more than one server and nothing above told them apart
    // (in none of them or in several, no link): refuse rather than give a
    // role in a server they didn't mean. The page points them to their
    // server's link. Usable in just one server: that's the only meaning left.
    let usable_guilds: Vec<&str> = usable.iter().map(|c| c.4.as_str()).collect();
    if !one_guild(&usable_guilds) {
        record_attempt(&state, &discord_id, ip_hash.as_deref(), &normalized, false).await?;
        return Ok(Json(json!({"error": "ambiguous_code"})));
    }
    let cand = usable[0];

    // Membership check before insert so we can persist the pending flag.
    // If the gateway is down, default to pending=false — under-reporting is
    // safer than blocking the redeem; the poller / sync event will reconcile.
    let pending = if let Some(guilds) = &member_of {
        !guilds.contains(&cand.4)
    } else {
        match auth_gateway::fetch_user_guild_ids(
            &state.http,
            &state.config.auth_gateway_url,
            &state.config.internal_api_key,
            &discord_id,
        )
        .await
        {
            Ok(guilds) => !guilds.contains(&cand.4),
            Err(e) => {
                tracing::warn!(discord_id, "auth_gateway membership check failed: {e}");
                false
            }
        }
    };

    let role_expires_at: Option<DateTime<Utc>> = cand
        .12
        .filter(|h| *h > 0)
        .map(|hours| Utc::now() + chrono::Duration::hours(hours as i64));

    // Atomic insert + counter bump.
    let mut tx = state.pool.begin().await?;
    let insert_res = sqlx::query(
        "INSERT INTO redemptions \
            (code_id, batch_id, guild_id, discord_id, ip_hash, user_agent, pending, role_expires_at) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8) ON CONFLICT (code_id, discord_id) DO NOTHING",
    )
    .bind(cand.0)
    .bind(cand.1)
    .bind(&cand.4)
    .bind(&discord_id)
    .bind(&ip_hash)
    .bind(&user_agent)
    .bind(pending)
    .bind(role_expires_at)
    .execute(&mut *tx)
    .await?;

    if insert_res.rows_affected() == 0 {
        tx.rollback().await?;
        record_attempt(&state, &discord_id, ip_hash.as_deref(), &normalized, false).await?;
        return Ok(Json(json!({"error": "already_redeemed_by_you"})));
    }

    sqlx::query("UPDATE codes SET uses_count = uses_count + 1 WHERE id = $1")
        .bind(cand.0)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;

    record_attempt(&state, &discord_id, ip_hash.as_deref(), &normalized, true).await?;

    let _ = state
        .player_sync_tx
        .send(PlayerSyncEvent {
            discord_id: discord_id.clone(),
        })
        .await;

    Ok(Json(json!({
        "success": true,
        "batch_name": cand.10,
        "guild_id": cand.4,
        "pending": pending,
        "invite_url": cand.11,
        "role_expires_at": role_expires_at,
    })))
}

async fn record_attempt(
    state: &AppState,
    discord_id: &str,
    ip_hash: Option<&str>,
    code: &str,
    success: bool,
) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO redemption_attempts (discord_id, ip_hash, attempted_code, success) \
         VALUES ($1,$2,$3,$4)",
    )
    .bind(discord_id)
    .bind(ip_hash)
    .bind(code)
    .bind(success)
    .execute(&state.pool)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn one_guild_means_every_candidate_shares_it() {
        assert!(one_guild(&[]));
        assert!(one_guild(&["A"]));
        assert!(one_guild(&["A", "A", "A"]));
        assert!(!one_guild(&["A", "B"]));
        assert!(!one_guild(&["A", "A", "B", "A"]));
    }

    #[test]
    fn membership_keeps_only_the_servers_the_person_is_in() {
        // PODCODE in A and B; they're in B: only B's codes stay.
        assert_eq!(
            member_guilds_mask(&["A", "B", "B"], &ids(&["B", "Z"])),
            [false, true, true]
        );
        // In both: both stay, and the caller refuses if both are usable.
        assert_eq!(
            member_guilds_mask(&["A", "B"], &ids(&["A", "B"])),
            [true, true]
        );
    }

    #[test]
    fn membership_in_none_of_them_keeps_every_server() {
        // Redeeming before joining: usability decides, or the caller refuses.
        assert_eq!(member_guilds_mask(&["A", "B"], &ids(&["Z"])), [true, true]);
        assert_eq!(member_guilds_mask(&["A", "B"], &[]), [true, true]);
    }
}
