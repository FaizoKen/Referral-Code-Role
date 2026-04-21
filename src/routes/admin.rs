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

#[derive(Serialize)]
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
    pub remaining: Option<i64>,
}

pub async fn list_batches(
    State(state): State<Arc<AppState>>,
    Query(q): Query<GuildQuery>,
    jar: CookieJar,
) -> Result<Json<Value>, AppError> {
    require_manager(&state, &jar, &q.guild_id).await?;

    let rows = sqlx::query_as::<
        _,
        (
            i64,
            String,
            Option<String>,
            String,
            Option<i32>,
            Option<i32>,
            Option<DateTime<Utc>>,
            Option<DateTime<Utc>>,
            DateTime<Utc>,
            Option<String>,
            Option<i32>,
        ),
    >(
        "SELECT id, name, description, kind, max_redemptions_per_code, max_redemptions_total, \
                expires_at, revoked_at, created_at, invite_url, role_duration_hours \
         FROM code_batches WHERE guild_id = $1 AND revoked_at IS NULL \
         ORDER BY created_at DESC",
    )
    .bind(&q.guild_id)
    .fetch_all(&state.pool)
    .await?;

    let mut views: Vec<BatchView> = Vec::with_capacity(rows.len());
    for r in rows {
        let total_codes: i64 =
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM codes WHERE batch_id = $1")
                .bind(r.0)
                .fetch_one(&state.pool)
                .await?;
        let total_redemptions: i64 =
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM redemptions WHERE batch_id = $1")
                .bind(r.0)
                .fetch_one(&state.pool)
                .await?;
        let remaining = r.5.map(|cap| (cap as i64) - total_redemptions);

        views.push(BatchView {
            id: r.0,
            name: r.1,
            description: r.2,
            kind: r.3,
            max_redemptions_per_code: r.4,
            max_redemptions_total: r.5,
            expires_at: r.6,
            revoked_at: r.7,
            created_at: r.8,
            invite_url: r.9,
            role_duration_hours: r.10,
            total_codes,
            total_redemptions,
            remaining,
        });
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
    if body.name.trim().is_empty() {
        return Err(AppError::BadRequest("Name required".into()));
    }

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
    .bind(body.name.trim())
    .bind(&body.description)
    .bind(&body.kind)
    .bind(body.max_redemptions_per_code)
    .bind(body.max_redemptions_total)
    .bind(body.expires_at)
    .bind(&admin_id)
    .bind(&invite_url)
    .bind(role_duration_hours)
    .fetch_one(&state.pool)
    .await
    .map_err(|e| match e {
        sqlx::Error::Database(db) if db.constraint().is_some() => {
            AppError::BadRequest("A batch with that name already exists".into())
        }
        other => AppError::Database(other),
    })?;

    Ok(Json(json!({ "id": id })))
}

#[derive(Deserialize)]
pub struct UpdateBatchBody {
    pub name: Option<String>,
    pub description: Option<String>,
    pub expires_at: Option<Option<DateTime<Utc>>>,
    pub max_redemptions_total: Option<Option<i32>>,
    pub invite_url: Option<Option<String>>,
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

    if let Some(name) = body.name {
        sqlx::query("UPDATE code_batches SET name = $1, updated_at = now() WHERE id = $2")
            .bind(name.trim())
            .bind(id)
            .execute(&state.pool)
            .await?;
    }
    if let Some(desc) = body.description {
        sqlx::query("UPDATE code_batches SET description = $1, updated_at = now() WHERE id = $2")
            .bind(desc)
            .bind(id)
            .execute(&state.pool)
            .await?;
    }
    if let Some(exp) = body.expires_at {
        sqlx::query("UPDATE code_batches SET expires_at = $1, updated_at = now() WHERE id = $2")
            .bind(exp)
            .bind(id)
            .execute(&state.pool)
            .await?;
    }
    if let Some(cap) = body.max_redemptions_total {
        sqlx::query(
            "UPDATE code_batches SET max_redemptions_total = $1, updated_at = now() WHERE id = $2",
        )
        .bind(cap)
        .bind(id)
        .execute(&state.pool)
        .await?;
    }
    if let Some(invite) = body.invite_url {
        let normalized = normalize_invite_url(invite)?;
        sqlx::query("UPDATE code_batches SET invite_url = $1, updated_at = now() WHERE id = $2")
            .bind(&normalized)
            .bind(id)
            .execute(&state.pool)
            .await?;
    }
    if let Some(dur) = body.role_duration_hours {
        let normalized = validate_role_duration(dur)?;
        sqlx::query(
            "UPDATE code_batches SET role_duration_hours = $1, updated_at = now() WHERE id = $2",
        )
        .bind(normalized)
        .bind(id)
        .execute(&state.pool)
        .await?;
    }

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

    if let Some(custom) = body.custom_codes {
        for raw in custom {
            let normalized = code_gen::normalize_code(&raw);
            if normalized.is_empty() {
                continue;
            }
            let res = sqlx::query("INSERT INTO codes (batch_id, code) VALUES ($1, $2) ON CONFLICT DO NOTHING")
                .bind(batch_id)
                .bind(&normalized)
                .execute(&state.pool)
                .await?;
            if res.rows_affected() > 0 {
                inserted.push(normalized);
            }
        }
    } else {
        let count = body.count.unwrap_or(0).clamp(0, 1000) as usize;
        let length = body.length.unwrap_or(12).clamp(6, 20) as usize;
        let prefix = body.prefix.as_deref();

        for _ in 0..count {
            let mut attempts = 0;
            loop {
                attempts += 1;
                let code = code_gen::generate_code(length, prefix);
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
    let search = q.q.unwrap_or_default();

    let rows = sqlx::query_as::<_, (i64, String, i32, Option<DateTime<Utc>>, DateTime<Utc>)>(
        "SELECT id, code, uses_count, revoked_at, created_at FROM codes \
         WHERE batch_id = $1 AND ($2 = '' OR code ILIKE '%' || $2 || '%') \
         ORDER BY created_at DESC LIMIT $3 OFFSET $4",
    )
    .bind(batch_id)
    .bind(&search)
    .bind(page_size)
    .bind(offset)
    .fetch_all(&state.pool)
    .await?;

    let total: i64 = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM codes WHERE batch_id = $1 AND ($2 = '' OR code ILIKE '%' || $2 || '%')",
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

    let target = format!(
        "{}/verify?code={}",
        state.config.base_url,
        urlencoding::encode(&code)
    );
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

    let rows = sqlx::query_as::<_, (i64, String, String, DateTime<Utc>)>(
        "SELECT r.id, r.discord_id, c.code, r.redeemed_at \
         FROM redemptions r JOIN codes c ON c.id = r.code_id \
         WHERE r.batch_id = $1 ORDER BY r.redeemed_at DESC LIMIT $2 OFFSET $3",
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
        .map(|(id, discord_id, code, redeemed_at)| {
            json!({
                "id": id,
                "discord_id": discord_id,
                "code": code,
                "redeemed_at": redeemed_at,
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

pub async fn stats(
    State(state): State<Arc<AppState>>,
    Query(q): Query<GuildQuery>,
    jar: CookieJar,
) -> Result<Json<Value>, AppError> {
    require_manager(&state, &jar, &q.guild_id).await?;

    let guild_name = auth_gateway::fetch_guild_name(
        &state.http,
        &state.config.auth_gateway_url,
        &state.config.internal_api_key,
        &q.guild_id,
    )
    .await
    .unwrap_or(None);

    let batches: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM code_batches WHERE guild_id = $1 AND revoked_at IS NULL",
    )
    .bind(&q.guild_id)
    .fetch_one(&state.pool)
    .await?;

    let total_codes: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM codes c JOIN code_batches b ON b.id = c.batch_id \
         WHERE b.guild_id = $1",
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

