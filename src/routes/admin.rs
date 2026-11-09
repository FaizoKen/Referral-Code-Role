use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use axum_extra::extract::cookie::Cookie;
use axum_extra::extract::CookieJar;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::error::AppError;
use crate::services::sync::ConfigSyncEvent;
use crate::services::{auth_gateway, code_gen, qr, session};
use crate::AppState;

const SESSION_COOKIE: &str = "rl_session";

#[derive(Deserialize)]
pub struct GuildQuery {
    pub guild_id: String,
}

async fn require_manager(
    state: &AppState,
    jar: &CookieJar,
    guild_id: &str,
) -> Result<String, AppError> {
    let cookie = jar.get(SESSION_COOKIE).ok_or(AppError::Unauthorized)?;
    let (discord_id, _) = session::verify_session(cookie.value(), &state.config.session_secret)
        .ok_or(AppError::Unauthorized)?;

    let cookie_for_forward = Cookie::new(SESSION_COOKIE, cookie.value().to_string());
    let cookie_header = cookie_for_forward.encoded().to_string();

    let is_manager = auth_gateway::check_guild_manager(
        &state.http,
        &state.config.auth_gateway_url,
        &cookie_header,
        guild_id,
    )
    .await?;

    if !is_manager {
        return Err(AppError::Forbidden);
    }
    Ok(discord_id)
}

async fn ensure_batch_in_guild(
    state: &AppState,
    batch_id: i64,
    guild_id: &str,
) -> Result<(), AppError> {
    let row = sqlx::query_scalar::<_, String>("SELECT guild_id FROM code_batches WHERE id = $1")
        .bind(batch_id)
        .fetch_optional(&state.pool)
        .await?;
    match row {
        Some(g) if g == guild_id => Ok(()),
        Some(_) => Err(AppError::Forbidden),
        None => Err(AppError::NotFound("Batch not found".into())),
    }
}

async fn ensure_code_in_guild(
    state: &AppState,
    code_id: i64,
    guild_id: &str,
) -> Result<i64, AppError> {
    let row = sqlx::query_as::<_, (i64, String)>(
        "SELECT b.id, b.guild_id FROM codes c \
         JOIN code_batches b ON b.id = c.batch_id \
         WHERE c.id = $1",
    )
    .bind(code_id)
    .fetch_optional(&state.pool)
    .await?;
    match row {
        Some((bid, g)) if g == guild_id => Ok(bid),
        Some(_) => Err(AppError::Forbidden),
        None => Err(AppError::NotFound("Code not found".into())),
    }
}

#[derive(Serialize, sqlx::FromRow)]
pub struct BatchView {
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
    pub kind: String,
    pub max_redemptions_per_code: Option<i32>,
    pub max_redemptions_total: Option<i32>,
    pub expires_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub invite_url: Option<String>,
    pub role_duration_hours: Option<i32>,
    pub total_codes: i64,
    pub total_redemptions: i64,
    /// How many of this server's roles have the group ticked in their
    /// RoleLogic settings. 0: its codes redeem, but give no role yet.
    pub role_links: i64,
    #[sqlx(skip)]
    pub remaining: Option<i64>,
}

pub async fn list_batches(
    State(state): State<Arc<AppState>>,
    Query(q): Query<GuildQuery>,
    jar: CookieJar,
) -> Result<Json<Value>, AppError> {
    require_manager(&state, &jar, &q.guild_id).await?;

    let mut views = sqlx::query_as::<_, BatchView>(
        "SELECT b.id, b.name, b.description, b.kind, b.max_redemptions_per_code, \
                b.max_redemptions_total, b.expires_at, b.revoked_at, b.created_at, \
                b.invite_url, b.role_duration_hours, \
                (SELECT COUNT(*) FROM codes c WHERE c.batch_id = b.id) AS total_codes, \
                (SELECT COUNT(*) FROM redemptions r WHERE r.batch_id = b.id) AS total_redemptions, \
                (SELECT COUNT(*) FROM role_links l WHERE l.guild_id = b.guild_id \
                    AND l.conditions->'approved_batch_ids' @> jsonb_build_array(b.id)) AS role_links \
         FROM code_batches b WHERE b.guild_id = $1 AND b.revoked_at IS NULL \
         ORDER BY b.created_at DESC",
    )
    .bind(&q.guild_id)
    .fetch_all(&state.pool)
    .await?;
    for v in &mut views {
        v.remaining = v.max_redemptions_total.map(|cap| cap as i64 - v.total_redemptions);
    }

    Ok(Json(json!({ "batches": views })))
}

#[derive(Deserialize)]
pub struct CreateBatchBody {
    pub name: String,
    pub description: Option<String>,
    pub kind: String,
    pub max_redemptions_per_code: Option<i32>,
    pub max_redemptions_total: Option<i32>,
    pub expires_at: Option<DateTime<Utc>>,
    pub invite_url: Option<String>,
    pub role_duration_hours: Option<i32>,
}

/// Longest group name: the dashboard lists groups as options, whose labels
/// it limits to 100 characters (with room for " (retired)").
const MAX_NAME_LEN: usize = 80;

fn validate_name(raw: &str) -> Result<String, AppError> {
    let name = raw.trim();
    if name.is_empty() {
        return Err(AppError::BadRequest("Give the group a name".into()));
    }
    if name.chars().count() > MAX_NAME_LEN {
        return Err(AppError::BadRequest(format!(
            "Keep the group name to {MAX_NAME_LEN} characters or fewer"
        )));
    }
    Ok(name.to_string())
}

/// A redemption cap: blank is no cap; 0 or less would stop every redeem.
fn validate_cap(cap: Option<i32>) -> Result<Option<i32>, AppError> {
    match cap {
        Some(n) if n < 1 => Err(AppError::BadRequest(
            "The redemption limit has to be at least 1 (leave it blank for no limit)".into(),
        )),
        other => Ok(other),
    }
}

/// A unique-name violation, said in the page's words.
fn name_taken(e: sqlx::Error) -> AppError {
    match e {
        sqlx::Error::Database(db) if db.constraint().is_some() => {
            AppError::BadRequest("You already have a group with that name".into())
        }
        other => AppError::Database(other),
    }
}

/// For PATCH bodies: a field left out is `None` (unchanged), a field sent as
/// `null` is `Some(None)` (cleared). Serde's own `Option<Option<T>>` reads
/// `null` as `None`, so without this a cleared expiry was silently kept.
fn double_option<'de, T, D>(de: D) -> Result<Option<Option<T>>, D::Error>
where
    T: Deserialize<'de>,
    D: serde::Deserializer<'de>,
{
    Option::<T>::deserialize(de).map(Some)
}

fn validate_role_duration(hours: Option<i32>) -> Result<Option<i32>, AppError> {
    match hours {
        None => Ok(None),
        Some(h) if h <= 0 => Ok(None),
        Some(h) if h > 24 * 365 * 10 => Err(AppError::BadRequest(
            "Role duration is too long (max 10 years)".into(),
        )),
        Some(h) => Ok(Some(h)),
    }
}

fn normalize_invite_url(raw: Option<String>) -> Result<Option<String>, AppError> {
    let Some(s) = raw else { return Ok(None) };
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    if !(trimmed.starts_with("https://") || trimmed.starts_with("http://")) {
        return Err(AppError::BadRequest(
            "Invite URL must start with https:// or http://".into(),
        ));
    }
    Ok(Some(trimmed.to_string()))
}

pub async fn create_batch(
    State(state): State<Arc<AppState>>,
    Query(q): Query<GuildQuery>,
    jar: CookieJar,
    Json(body): Json<CreateBatchBody>,
) -> Result<Json<Value>, AppError> {
    let admin_id = require_manager(&state, &jar, &q.guild_id).await?;

    if !["unique_per_code", "unique_per_user", "shared_unlimited"].contains(&body.kind.as_str()) {
        return Err(AppError::BadRequest("Invalid kind".into()));
    }
    let name = validate_name(&body.name)?;
    let max_redemptions_total = validate_cap(body.max_redemptions_total)?;
    let description = body
        .description
        .map(|d| d.trim().to_string())
        .filter(|d| !d.is_empty());

    let invite_url = normalize_invite_url(body.invite_url)?;
    let role_duration_hours = validate_role_duration(body.role_duration_hours)?;

    let id: i64 = sqlx::query_scalar(
        "INSERT INTO code_batches \
            (guild_id, name, description, kind, max_redemptions_per_code, \
             max_redemptions_total, expires_at, created_by_discord_id, invite_url, \
             role_duration_hours) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10) RETURNING id",
    )
    .bind(&q.guild_id)
    .bind(&name)
    .bind(&description)
    .bind(&body.kind)
    .bind(body.max_redemptions_per_code)
    .bind(max_redemptions_total)
    .bind(body.expires_at)
    .bind(&admin_id)
    .bind(&invite_url)
    .bind(role_duration_hours)
    .fetch_one(&state.pool)
    .await
    .map_err(name_taken)?;

    Ok(Json(json!({ "id": id })))
}

/// The admin page's Edit form. The group's type can't change: codes were
/// made, shared and maybe redeemed under it.
#[derive(Deserialize)]
pub struct UpdateBatchBody {
    pub name: Option<String>,
    #[serde(default, deserialize_with = "double_option")]
    pub description: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub expires_at: Option<Option<DateTime<Utc>>>,
    #[serde(default, deserialize_with = "double_option")]
    pub max_redemptions_total: Option<Option<i32>>,
    #[serde(default, deserialize_with = "double_option")]
    pub invite_url: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub role_duration_hours: Option<Option<i32>>,
}

pub async fn update_batch(
    State(state): State<Arc<AppState>>,
    Query(q): Query<GuildQuery>,
    Path(id): Path<i64>,
    jar: CookieJar,
    Json(body): Json<UpdateBatchBody>,
) -> Result<Json<Value>, AppError> {
    require_manager(&state, &jar, &q.guild_id).await?;
    ensure_batch_in_guild(&state, id, &q.guild_id).await?;

    // Check everything before writing anything, then write it all at once.
    let name = body.name.as_deref().map(validate_name).transpose()?;
    let description = body
        .description
        .map(|d| d.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()));
    let cap = body.max_redemptions_total.map(validate_cap).transpose()?;
    let invite = body.invite_url.map(normalize_invite_url).transpose()?;
    let duration = body
        .role_duration_hours
        .map(validate_role_duration)
        .transpose()?;

    // Each `$n_set` says whether the field was sent; an unsent one keeps
    // its value. A role duration applies to codes redeemed from now on:
    // earlier redemptions keep the end time they were given.
    sqlx::query(
        "UPDATE code_batches SET \
            name = CASE WHEN $2 THEN $3 ELSE name END, \
            description = CASE WHEN $4 THEN $5 ELSE description END, \
            expires_at = CASE WHEN $6 THEN $7 ELSE expires_at END, \
            max_redemptions_total = CASE WHEN $8 THEN $9 ELSE max_redemptions_total END, \
            invite_url = CASE WHEN $10 THEN $11 ELSE invite_url END, \
            role_duration_hours = CASE WHEN $12 THEN $13 ELSE role_duration_hours END, \
            updated_at = now() \
         WHERE id = $1",
    )
    .bind(id)
    .bind(name.is_some())
    .bind(name.unwrap_or_default())
    .bind(description.is_some())
    .bind(description.flatten())
    .bind(body.expires_at.is_some())
    .bind(body.expires_at.flatten())
    .bind(cap.is_some())
    .bind(cap.flatten())
    .bind(invite.is_some())
    .bind(invite.flatten())
    .bind(duration.is_some())
    .bind(duration.flatten())
    .execute(&state.pool)
    .await
    .map_err(name_taken)?;

    Ok(Json(json!({"success": true})))
}

pub async fn revoke_batch(
    State(state): State<Arc<AppState>>,
    Query(q): Query<GuildQuery>,
    Path(id): Path<i64>,
    jar: CookieJar,
) -> Result<Json<Value>, AppError> {
    require_manager(&state, &jar, &q.guild_id).await?;
    ensure_batch_in_guild(&state, id, &q.guild_id).await?;

    sqlx::query("UPDATE code_batches SET revoked_at = now(), updated_at = now() WHERE id = $1")
        .bind(id)
        .execute(&state.pool)
        .await?;

    let role_links = sqlx::query_as::<_, (String, String)>(
        "SELECT guild_id, role_id FROM role_links WHERE guild_id = $1",
    )
    .bind(&q.guild_id)
    .fetch_all(&state.pool)
    .await?;
    for (gid, rid) in role_links {
        let _ = state
            .config_sync_tx
            .send(ConfigSyncEvent {
                guild_id: gid,
                role_id: rid,
            })
            .await;
    }

    Ok(Json(json!({"success": true})))
}

#[derive(Deserialize)]
pub struct GenerateCodesBody {
    pub count: Option<i32>,
    pub length: Option<i32>,
    pub prefix: Option<String>,
    /// Hyphen after every N random characters (3, 4 or 5); absent or 0 = none.
    pub group: Option<u32>,
    pub custom_codes: Option<Vec<String>>,
}

pub async fn generate_codes(
    State(state): State<Arc<AppState>>,
    Query(q): Query<GuildQuery>,
    Path(batch_id): Path<i64>,
    jar: CookieJar,
    Json(body): Json<GenerateCodesBody>,
) -> Result<Json<Value>, AppError> {
    require_manager(&state, &jar, &q.guild_id).await?;
    ensure_batch_in_guild(&state, batch_id, &q.guild_id).await?;

    let mut inserted: Vec<String> = Vec::new();

    // ON CONFLICT covers both unique indexes: the stored text, and the code
    // key (a code that differs from one already in the group only by hyphens
    // is the same code, so it is skipped rather than stored twice).
    if let Some(custom) = body.custom_codes {
        // Check every line before writing any, so a bad paste adds nothing.
        let mut codes = Vec::with_capacity(custom.len());
        let mut problems = Vec::new();
        for raw in &custom {
            match code_gen::custom_code(raw) {
                Ok(Some(code)) => codes.push(code),
                Ok(None) => {}
                Err(e) => problems.push(e),
            }
        }
        if !problems.is_empty() {
            let more = problems.len().saturating_sub(3);
            let mut msg = problems.into_iter().take(3).collect::<Vec<_>>().join(" ");
            if more > 0 {
                msg.push_str(&format!(" (and {more} more)"));
            }
            return Err(AppError::BadRequest(msg));
        }
        for code in codes {
            let res = sqlx::query(
                "INSERT INTO codes (batch_id, code) VALUES ($1, $2) ON CONFLICT DO NOTHING",
            )
            .bind(batch_id)
            .bind(&code)
            .execute(&state.pool)
            .await?;
            if res.rows_affected() > 0 {
                inserted.push(code);
            }
        }
    } else {
        let count = body.count.unwrap_or(0).clamp(0, 1000) as usize;
        let length = body
            .length
            .unwrap_or(12)
            .clamp(6, code_gen::MAX_BODY_LEN as i32) as usize;
        let prefix = match body.prefix.as_deref() {
            Some(raw) => code_gen::prefix(raw).map_err(AppError::BadRequest)?,
            None => None,
        };
        let prefix = prefix.as_deref();
        let group = code_gen::group_size(body.group).map_err(AppError::BadRequest)?;

        for _ in 0..count {
            let mut attempts = 0;
            loop {
                attempts += 1;
                let code = code_gen::generate_code(length, prefix, group);
                let res = sqlx::query(
                    "INSERT INTO codes (batch_id, code) VALUES ($1, $2) ON CONFLICT DO NOTHING",
                )
                .bind(batch_id)
                .bind(&code)
                .execute(&state.pool)
                .await?;
                if res.rows_affected() > 0 {
                    inserted.push(code);
                    break;
                }
                if attempts >= 5 {
                    return Err(AppError::BadRequest(
                        "Code length too short for the requested count (collisions)".into(),
                    ));
                }
            }
        }
    }

    Ok(Json(json!({ "inserted": inserted.len(), "codes": inserted })))
}

#[derive(Deserialize)]
pub struct ListCodesQuery {
    pub guild_id: String,
    pub page: Option<i64>,
    pub page_size: Option<i64>,
    pub q: Option<String>,
}

pub async fn list_codes(
    State(state): State<Arc<AppState>>,
    Query(q): Query<ListCodesQuery>,
    Path(batch_id): Path<i64>,
    jar: CookieJar,
) -> Result<Json<Value>, AppError> {
    require_manager(&state, &jar, &q.guild_id).await?;
    ensure_batch_in_guild(&state, batch_id, &q.guild_id).await?;

    let page = q.page.unwrap_or(1).max(1);
    let page_size = q.page_size.unwrap_or(50).clamp(1, 500);
    let offset = (page - 1) * page_size;
    // Search the way codes redeem: "LAT7XK" finds "LAT-7XK2M9QH4PRT".
    let search = code_gen::code_key(&q.q.unwrap_or_default());

    let rows = sqlx::query_as::<_, (i64, String, i32, Option<DateTime<Utc>>, DateTime<Utc>)>(
        "SELECT id, code, uses_count, revoked_at, created_at FROM codes \
         WHERE batch_id = $1 AND ($2 = '' OR code_key LIKE '%' || $2 || '%') \
         ORDER BY created_at DESC LIMIT $3 OFFSET $4",
    )
    .bind(batch_id)
    .bind(&search)
    .bind(page_size)
    .bind(offset)
    .fetch_all(&state.pool)
    .await?;

    let total: i64 = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM codes WHERE batch_id = $1 AND ($2 = '' OR code_key LIKE '%' || $2 || '%')",
    )
    .bind(batch_id)
    .bind(&search)
    .fetch_one(&state.pool)
    .await?;

    let codes: Vec<Value> = rows
        .into_iter()
        .map(|(id, code, uses, revoked_at, created_at)| {
            json!({
                "id": id,
                "code": code,
                "uses_count": uses,
                "revoked_at": revoked_at,
                "created_at": created_at,
            })
        })
        .collect();

    Ok(Json(json!({
        "codes": codes,
        "page": page,
        "page_size": page_size,
        "total": total
    })))
}

pub async fn revoke_code(
    State(state): State<Arc<AppState>>,
    Query(q): Query<GuildQuery>,
    Path(id): Path<i64>,
    jar: CookieJar,
) -> Result<Json<Value>, AppError> {
    require_manager(&state, &jar, &q.guild_id).await?;
    ensure_code_in_guild(&state, id, &q.guild_id).await?;

    sqlx::query("UPDATE codes SET revoked_at = now() WHERE id = $1")
        .bind(id)
        .execute(&state.pool)
        .await?;

    Ok(Json(json!({"success": true})))
}

pub async fn code_qr(
    State(state): State<Arc<AppState>>,
    Query(q): Query<GuildQuery>,
    Path(id): Path<i64>,
    jar: CookieJar,
) -> Result<Response, AppError> {
    require_manager(&state, &jar, &q.guild_id).await?;
    ensure_code_in_guild(&state, id, &q.guild_id).await?;

    let code: String = sqlx::query_scalar("SELECT code FROM codes WHERE id = $1")
        .bind(id)
        .fetch_one(&state.pool)
        .await?;

    let target = redeem_link(&state.config.base_url, &code, &q.guild_id);
    let svg = qr::render_svg(&target)?;
    Ok((
        StatusCode::OK,
        [(header::CONTENT_TYPE, "image/svg+xml")],
        svg,
    )
        .into_response())
}

pub async fn list_redemptions(
    State(state): State<Arc<AppState>>,
    Query(q): Query<ListCodesQuery>,
    Path(batch_id): Path<i64>,
    jar: CookieJar,
) -> Result<Json<Value>, AppError> {
    require_manager(&state, &jar, &q.guild_id).await?;
    ensure_batch_in_guild(&state, batch_id, &q.guild_id).await?;

    let page = q.page.unwrap_or(1).max(1);
    let page_size = q.page_size.unwrap_or(50).clamp(1, 500);
    let offset = (page - 1) * page_size;

    let rows = sqlx::query_as::<_, RedemptionRow>(
        "SELECT r.id, r.discord_id, r.display_name, c.code, r.redeemed_at, r.role_expires_at, r.pending \
         FROM redemptions r JOIN codes c ON c.id = r.code_id \
         WHERE r.batch_id = $1 ORDER BY r.redeemed_at DESC, r.id DESC LIMIT $2 OFFSET $3",
    )
    .bind(batch_id)
    .bind(page_size)
    .bind(offset)
    .fetch_all(&state.pool)
    .await?;

    let total: i64 =
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM redemptions WHERE batch_id = $1")
            .bind(batch_id)
            .fetch_one(&state.pool)
            .await?;

    let items: Vec<Value> = rows
        .into_iter()
        .map(|r| {
            json!({
                "id": r.id,
                "discord_id": r.discord_id,
                "display_name": r.display_name,
                "code": r.code,
                "redeemed_at": r.redeemed_at,
                "role_expires_at": r.role_expires_at,
                "pending": r.pending,
            })
        })
        .collect();

    Ok(Json(json!({
        "redemptions": items,
        "page": page,
        "page_size": page_size,
        "total": total,
    })))
}

#[derive(sqlx::FromRow)]
struct RedemptionRow {
    id: i64,
    discord_id: String,
    display_name: Option<String>,
    code: String,
    redeemed_at: DateTime<Utc>,
    role_expires_at: Option<DateTime<Utc>>,
    pending: bool,
}

/// One CSV field: quoted when it holds a comma, quote or line break, and
/// defused when a spreadsheet would read it as a formula (a member's display
/// name is theirs to choose: "=HYPERLINK(…)" must stay text).
fn csv_field(raw: &str) -> String {
    let s = if raw.starts_with(['=', '+', '-', '@', '\t', '\r']) {
        format!("'{raw}")
    } else {
        raw.to_string()
    };
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s
    }
}

fn csv_line(fields: &[&str]) -> String {
    let mut line = fields.iter().map(|f| csv_field(f)).collect::<Vec<_>>().join(",");
    line.push_str("\r\n");
    line
}

/// A download name from a group's name: letters, digits and dashes.
fn file_stem(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    let out = out.trim_end_matches('-').to_string();
    if out.is_empty() {
        "code-group".into()
    } else {
        out
    }
}

fn csv_response(stem: &str, suffix: &str, body: String) -> Response {
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "text/csv; charset=utf-8".to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{stem}-{suffix}.csv\""),
            ),
            (header::CACHE_CONTROL, "no-store".to_string()),
        ],
        // A BOM, so Excel reads names in any script as UTF-8.
        format!("\u{feff}{body}"),
    )
        .into_response()
}

async fn batch_name(state: &AppState, batch_id: i64) -> Result<String, AppError> {
    Ok(sqlx::query_scalar("SELECT name FROM code_batches WHERE id = $1")
        .bind(batch_id)
        .fetch_one(&state.pool)
        .await?)
}

/// Every code in a group with its own redemption link (the one Copy URL and
/// the QR carry), for mail-merges and printers: one-time codes come in
/// hundreds, and the page shows them 50 at a time.
pub async fn codes_csv(
    State(state): State<Arc<AppState>>,
    Query(q): Query<GuildQuery>,
    Path(batch_id): Path<i64>,
    jar: CookieJar,
) -> Result<Response, AppError> {
    require_manager(&state, &jar, &q.guild_id).await?;
    ensure_batch_in_guild(&state, batch_id, &q.guild_id).await?;
    let name = batch_name(&state, batch_id).await?;

    let rows = sqlx::query_as::<_, (String, i32, Option<DateTime<Utc>>, DateTime<Utc>)>(
        "SELECT code, uses_count, revoked_at, created_at FROM codes \
         WHERE batch_id = $1 ORDER BY created_at, id",
    )
    .bind(batch_id)
    .fetch_all(&state.pool)
    .await?;

    let mut body = csv_line(&["code", "redeem_link", "times_used", "status", "created_at"]);
    for (code, uses, revoked_at, created_at) in rows {
        let link = redeem_link(&state.config.base_url, &code, &q.guild_id);
        body.push_str(&csv_line(&[
            &code,
            &link,
            &uses.to_string(),
            if revoked_at.is_some() { "retired" } else { "active" },
            &created_at.to_rfc3339(),
        ]));
    }
    Ok(csv_response(&file_stem(&name), "codes", body))
}

/// Who redeemed a group's codes, newest first.
pub async fn redemptions_csv(
    State(state): State<Arc<AppState>>,
    Query(q): Query<GuildQuery>,
    Path(batch_id): Path<i64>,
    jar: CookieJar,
) -> Result<Response, AppError> {
    require_manager(&state, &jar, &q.guild_id).await?;
    ensure_batch_in_guild(&state, batch_id, &q.guild_id).await?;
    let name = batch_name(&state, batch_id).await?;

    let rows = sqlx::query_as::<_, RedemptionRow>(
        "SELECT r.id, r.discord_id, r.display_name, c.code, r.redeemed_at, r.role_expires_at, r.pending \
         FROM redemptions r JOIN codes c ON c.id = r.code_id \
         WHERE r.batch_id = $1 ORDER BY r.redeemed_at DESC, r.id DESC",
    )
    .bind(batch_id)
    .fetch_all(&state.pool)
    .await?;

    let mut body = csv_line(&[
        "redeemed_at",
        "discord_name",
        "discord_id",
        "code",
        "role_ends_at",
        "joined_server",
    ]);
    for r in rows {
        body.push_str(&csv_line(&[
            &r.redeemed_at.to_rfc3339(),
            r.display_name.as_deref().unwrap_or(""),
            &r.discord_id,
            &r.code,
            &r.role_expires_at.map(|t| t.to_rfc3339()).unwrap_or_default(),
            if r.pending { "not yet" } else { "yes" },
        ]));
    }
    Ok(csv_response(&file_stem(&name), "redemptions", body))
}

/// A code's own redemption link: it fills the code in and names the server,
/// so a code another server also uses still redeems here.
fn redeem_link(base_url: &str, code: &str, guild_id: &str) -> String {
    format!(
        "{}/verify?code={}&guild={}",
        base_url,
        urlencoding::encode(code),
        urlencoding::encode(guild_id)
    )
}

/// How long a server's name is reused before asking the gateway again. The
/// gateway's answer carries the server's whole member list, so the admin
/// page (which asks on every load, create and generate) must not ask often.
const GUILD_NAME_TTL: std::time::Duration = std::time::Duration::from_secs(10 * 60);

async fn guild_name(state: &AppState, guild_id: &str) -> Option<String> {
    if let Some((name, at)) = state.guild_names.lock().ok()?.get(guild_id).cloned() {
        if at.elapsed() < GUILD_NAME_TTL {
            return name;
        }
    }
    let name = auth_gateway::fetch_guild_name(
        &state.http,
        &state.config.auth_gateway_url,
        &state.config.internal_api_key,
        guild_id,
    )
    .await
    .ok()?;
    if let Ok(mut cache) = state.guild_names.lock() {
        if cache.len() > 10_000 {
            cache.clear();
        }
        cache.insert(guild_id.to_string(), (name.clone(), std::time::Instant::now()));
    }
    name
}

pub async fn stats(
    State(state): State<Arc<AppState>>,
    Query(q): Query<GuildQuery>,
    jar: CookieJar,
) -> Result<Json<Value>, AppError> {
    require_manager(&state, &jar, &q.guild_id).await?;

    let guild_name = guild_name(&state, &q.guild_id).await;

    let batches: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM code_batches WHERE guild_id = $1 AND revoked_at IS NULL",
    )
    .bind(&q.guild_id)
    .fetch_one(&state.pool)
    .await?;

    // Codes in the groups the page lists (a retired group's codes no longer work).
    let total_codes: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM codes c JOIN code_batches b ON b.id = c.batch_id \
         WHERE b.guild_id = $1 AND b.revoked_at IS NULL",
    )
    .bind(&q.guild_id)
    .fetch_one(&state.pool)
    .await?;

    let total_redemptions: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM redemptions WHERE guild_id = $1")
            .bind(&q.guild_id)
            .fetch_one(&state.pool)
            .await?;

    let last_24h: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM redemptions WHERE guild_id = $1 AND redeemed_at > now() - interval '1 day'",
    )
    .bind(&q.guild_id)
    .fetch_one(&state.pool)
    .await?;

    Ok(Json(json!({
        "guild_name": guild_name,
        "batches": batches,
        "codes": total_codes,
        "redemptions": total_redemptions,
        "redemptions_last_24h": last_24h,
    })))
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_fields_are_quoted_and_formulas_defused() {
        assert_eq!(csv_field("PP-7XK2-M9QH"), "PP-7XK2-M9QH");
        assert_eq!(csv_field("Kai, the builder"), "\"Kai, the builder\"");
        assert_eq!(csv_field("say \"hi\""), "\"say \"\"hi\"\"\"");
        assert_eq!(csv_field("=HYPERLINK(\"x\")"), "\"'=HYPERLINK(\"\"x\"\")\"");
        assert_eq!(csv_field("@kai"), "'@kai");
        assert_eq!(csv_line(&["a", "b"]), "a,b\r\n");
    }

    #[test]
    fn download_names_are_plain() {
        assert_eq!(file_stem("Kickstarter Backers"), "kickstarter-backers");
        assert_eq!(file_stem("Podcast Ep. 47 (Nov)"), "podcast-ep-47-nov");
        assert_eq!(file_stem("ポッドキャスト"), "code-group");
    }

    #[test]
    fn a_patch_tells_left_out_from_cleared() {
        let b: UpdateBatchBody = serde_json::from_str(r#"{"expires_at": null, "name": "X"}"#).unwrap();
        assert_eq!(b.expires_at, Some(None));
        assert!(b.max_redemptions_total.is_none());
        assert!(b.invite_url.is_none());
        let b: UpdateBatchBody = serde_json::from_str(r#"{"max_redemptions_total": 50}"#).unwrap();
        assert_eq!(b.max_redemptions_total, Some(Some(50)));
        assert!(b.expires_at.is_none());
    }

    #[test]
    fn caps_and_names_are_checked() {
        assert!(validate_cap(Some(0)).is_err());
        assert_eq!(validate_cap(None).unwrap(), None);
        assert_eq!(validate_cap(Some(100)).unwrap(), Some(100));
        assert!(validate_name("   ").is_err());
        assert!(validate_name(&"x".repeat(MAX_NAME_LEN + 1)).is_err());
        assert_eq!(validate_name("  Kickstarter Backers ").unwrap(), "Kickstarter Backers");
    }
}
