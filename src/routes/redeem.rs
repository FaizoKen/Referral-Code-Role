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

/// What a group's limits need to know about one more redemption of one of
/// its codes, by someone who hasn't redeemed that code before.
#[derive(Debug, Clone, Copy)]
struct Usage<'a> {
    kind: &'a str,
    /// Redemptions of this code so far.
    uses: i32,
    per_code_cap: Option<i32>,
    total_cap: Option<i32>,
    /// Redemptions in the whole group so far (read only when it has a cap).
    total: i64,
    /// This member has already redeemed some code in the group.
    claimed_in_group: bool,
}

/// Which of a group's limits one more redemption would break, if any. Pure,
/// so the redeem path asks it twice: once to choose a code, and again inside
/// the transaction with counts read under a lock.
///
/// - One-time codes: a code works once, for one person.
/// - Per-person codes: a code works for many people, but each person once
///   across the whole group (their second wristband code is refused).
/// - Single shared code: anyone, once each, until the group's cap.
fn limit_reason(u: &Usage) -> Option<&'static str> {
    if u.kind == "unique_per_user" && u.claimed_in_group {
        return Some("already_claimed_in_group");
    }
    if let Some(cap) = u.total_cap {
        if u.total >= cap as i64 {
            return Some("exhausted");
        }
    }
    if u.kind == "unique_per_code" && u.uses >= 1 {
        return Some("already_used");
    }
    if matches!(u.kind, "unique_per_code" | "unique_per_user") {
        if let Some(per_cap) = u.per_code_cap {
            if u.uses >= per_cap {
                return Some("already_used");
            }
        }
    }
    None
}

/// Reads what [`limit_reason`] needs. `uses` is passed in because the
/// transaction reads it under its own row lock.
async fn usage<'a>(
    conn: &mut sqlx::PgConnection,
    cand: &'a Candidate,
    uses: i32,
    discord_id: &str,
) -> Result<Usage<'a>, AppError> {
    let total: i64 = if cand.7.is_some() {
        sqlx::query_scalar("SELECT COUNT(*) FROM redemptions WHERE batch_id = $1")
            .bind(cand.1)
            .fetch_one(&mut *conn)
            .await?
    } else {
        0
    };
    let claimed_in_group: bool = if cand.5 == "unique_per_user" {
        sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM redemptions WHERE batch_id = $1 AND discord_id = $2)",
        )
        .bind(cand.1)
        .bind(discord_id)
        .fetch_one(&mut *conn)
        .await?
    } else {
        false
    };
    Ok(Usage {
        kind: cand.5.as_str(),
        uses,
        per_code_cap: cand.6,
        total_cap: cand.7,
        total,
        claimed_in_group,
    })
}

/// Why `discord_id` can't redeem `cand` right now, or `None` if they can.
async fn unusable_reason(
    state: &AppState,
    cand: &Candidate,
    discord_id: &str,
    now: DateTime<Utc>,
) -> Result<Option<&'static str>, AppError> {
    let mut conn = state.pool.acquire().await?;
    // Someone typing a code they already redeemed hears that, whatever else
    // is true of the code now (used up, retired, expired): their role is set.
    let already: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM redemptions WHERE code_id = $1 AND discord_id = $2)",
    )
    .bind(cand.0)
    .bind(discord_id)
    .fetch_one(&mut *conn)
    .await?;
    if already {
        return Ok(Some("already_redeemed_by_you"));
    }
    if cand.3.is_some() {
        return Ok(Some("invalid"));
    }
    if let Some(exp) = cand.8 {
        if exp < now {
            return Ok(Some("expired"));
        }
    }
    let u = usage(&mut conn, cand, cand.2, discord_id).await?;
    Ok(limit_reason(&u))
}

/// How many of a server's role links have this group ticked: 0 means a
/// redeem is recorded but gives no role until an admin ticks the group.
pub async fn approving_role_links(
    pool: &sqlx::PgPool,
    guild_id: &str,
    batch_id: i64,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT COUNT(*) FROM role_links \
         WHERE guild_id = $1 AND conditions->'approved_batch_ids' @> jsonb_build_array($2::bigint)",
    )
    .bind(guild_id)
    .bind(batch_id)
    .fetch_one(pool)
    .await
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
    let (discord_id, display_name) =
        session::verify_session(cookie.value(), &state.config.session_secret)
            .ok_or(AppError::Unauthorized)?;
    let display_name = Some(display_name.trim().to_string()).filter(|n| !n.is_empty());

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

    // The checks above ran outside this transaction, so two people redeeming
    // the last use of a one-time code (or the last place under a group's cap,
    // or one member redeeming two wristband codes at once) could both pass
    // them. Lock the code's row, and the group's row when a group-wide limit
    // applies, then check again against what is committed now. Code first,
    // group second, always: two redeems never wait on each other in opposite
    // orders. NO KEY UPDATE leaves the foreign-key checks of other inserts
    // into the group unblocked.
    let uses: i32 =
        sqlx::query_scalar("SELECT uses_count FROM codes WHERE id = $1 FOR NO KEY UPDATE")
            .bind(cand.0)
            .fetch_one(&mut *tx)
            .await?;
    if cand.7.is_some() || cand.5 == "unique_per_user" {
        sqlx::query("SELECT 1 FROM code_batches WHERE id = $1 FOR NO KEY UPDATE")
            .bind(cand.1)
            .execute(&mut *tx)
            .await?;
    }
    let locked = usage(&mut tx, cand, uses, &discord_id).await?;
    if let Some(reason) = limit_reason(&locked) {
        tx.rollback().await?;
        record_attempt(&state, &discord_id, ip_hash.as_deref(), &normalized, false).await?;
        return Ok(Json(json!({"error": reason})));
    }

    let insert_res = sqlx::query(
        "INSERT INTO redemptions \
            (code_id, batch_id, guild_id, discord_id, ip_hash, user_agent, pending, role_expires_at, display_name) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9) ON CONFLICT (code_id, discord_id) DO NOTHING",
    )
    .bind(cand.0)
    .bind(cand.1)
    .bind(&cand.4)
    .bind(&discord_id)
    .bind(&ip_hash)
    .bind(&user_agent)
    .bind(pending)
    .bind(role_expires_at)
    .bind(&display_name)
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

    // A group nobody has ticked in a role's settings records the redeem but
    // gives no role yet; say so instead of promising one. (If the lookup
    // fails, assume ticked: that was the page's only message before.)
    let role_ready = approving_role_links(&state.pool, &cand.4, cand.1)
        .await
        .map(|n| n > 0)
        .unwrap_or(true);

    Ok(Json(json!({
        "success": true,
        "batch_name": cand.10,
        "guild_id": cand.4,
        "pending": pending,
        "invite_url": cand.11,
        "role_expires_at": role_expires_at,
        "role_ready": role_ready,
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

    fn fresh(kind: &str) -> Usage<'_> {
        Usage {
            kind,
            uses: 0,
            per_code_cap: None,
            total_cap: None,
            total: 0,
            claimed_in_group: false,
        }
    }

    #[test]
    fn a_one_time_code_works_once() {
        assert_eq!(limit_reason(&fresh("unique_per_code")), None);
        let used = Usage { uses: 1, ..fresh("unique_per_code") };
        assert_eq!(limit_reason(&used), Some("already_used"));
        // A second one-time code from the same group is a different reward.
        let other = Usage { claimed_in_group: true, ..fresh("unique_per_code") };
        assert_eq!(limit_reason(&other), None);
    }

    #[test]
    fn per_person_codes_are_shared_but_once_per_person_per_group() {
        // The wristband code has been used by forty people: still fine.
        let busy = Usage { uses: 40, ..fresh("unique_per_user") };
        assert_eq!(limit_reason(&busy), None);
        // This member already redeemed one of the group's codes (any of them).
        let again = Usage { uses: 40, claimed_in_group: true, ..fresh("unique_per_user") };
        assert_eq!(limit_reason(&again), Some("already_claimed_in_group"));
        // A per-code cap still applies when a group carries one.
        let capped = Usage { uses: 5, per_code_cap: Some(5), ..fresh("unique_per_user") };
        assert_eq!(limit_reason(&capped), Some("already_used"));
    }

    #[test]
    fn a_shared_code_runs_until_the_group_cap() {
        let open = Usage { uses: 99, total: 99, total_cap: Some(100), ..fresh("shared_unlimited") };
        assert_eq!(limit_reason(&open), None);
        let full = Usage { total: 100, ..open };
        assert_eq!(limit_reason(&full), Some("exhausted"));
        // A shared code has no per-person-per-group rule: the same person on
        // a second shared code of the group is allowed (each code once).
        let second = Usage { claimed_in_group: true, ..fresh("shared_unlimited") };
        assert_eq!(limit_reason(&second), None);
    }

    #[test]
    fn a_member_who_already_claimed_hears_that_before_the_cap() {
        let both = Usage { total: 100, total_cap: Some(100), claimed_in_group: true, ..fresh("unique_per_user") };
        assert_eq!(limit_reason(&both), Some("already_claimed_in_group"));
    }

    #[test]
    fn membership_in_none_of_them_keeps_every_server() {
        // Redeeming before joining: usability decides, or the caller refuses.
        assert_eq!(member_guilds_mask(&["A", "B"], &ids(&["Z"])), [true, true]);
        assert_eq!(member_guilds_mask(&["A", "B"], &[]), [true, true]);
    }
}
