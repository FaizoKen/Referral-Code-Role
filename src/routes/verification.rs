use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use axum::Json;
use axum_extra::extract::CookieJar;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::error::AppError;
use crate::services::{session, sync};
use crate::AppState;

const SESSION_COOKIE: &str = "rl_session";

fn get_session(jar: &CookieJar, secret: &str) -> Result<(String, String), AppError> {
    let cookie = jar.get(SESSION_COOKIE).ok_or(AppError::Unauthorized)?;
    session::verify_session(cookie.value(), secret).ok_or(AppError::Unauthorized)
}

pub async fn verify_page(State(state): State<Arc<AppState>>) -> Response {
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        state.verify_html.clone(),
    )
        .into_response()
}

#[derive(Deserialize)]
pub struct LoginQuery {
    pub code: Option<String>,
}

pub async fn login(Query(q): Query<LoginQuery>) -> Result<Redirect, AppError> {
    let return_to = match q.code {
        Some(c) if !c.is_empty() => format!(
            "/referral-code-role/verify?code={}",
            urlencoding::encode(&c)
        ),
        _ => "/referral-code-role/verify".to_string(),
    };
    let url = format!("/auth/login?return_to={}", urlencoding::encode(&return_to));
    Ok(Redirect::temporary(&url))
}

pub async fn status(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
) -> Result<Json<Value>, AppError> {
    match get_session(&jar, &state.config.session_secret) {
        Ok((discord_id, display_name)) => {
            let payload = build_status_payload(&state, &discord_id, &display_name).await?;
            Ok(Json(payload))
        }
        Err(_) => Ok(Json(json!({"logged_in": false}))),
    }
}

pub async fn refresh(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
) -> Result<Json<Value>, AppError> {
    let (discord_id, display_name) = get_session(&jar, &state.config.session_secret)?;

    // Inline sync — bounded work (one gateway call + a few SQL queries).
    if let Err(e) = sync::sync_for_player(&discord_id, &state).await {
        tracing::warn!(discord_id, "Inline sync_for_player failed: {e}");
    }

    let payload = build_status_payload(&state, &discord_id, &display_name).await?;
    Ok(Json(payload))
}

async fn build_status_payload(
    state: &AppState,
    discord_id: &str,
    display_name: &str,
) -> Result<Value, AppError> {
    let recent = sqlx::query_as::<
        _,
        (String, String, DateTime<Utc>, Option<DateTime<Utc>>),
    >(
        "SELECT b.name, c.code, r.redeemed_at, r.role_expires_at FROM redemptions r \
         JOIN codes c ON c.id = r.code_id \
         JOIN code_batches b ON b.id = r.batch_id \
         WHERE r.discord_id = $1 ORDER BY r.redeemed_at DESC LIMIT 10",
    )
    .bind(discord_id)
    .fetch_all(&state.pool)
    .await?;

    let recent_json: Vec<Value> = recent
        .into_iter()
        .map(|(batch, code, ts, expires)| {
            json!({
                "batch": batch,
                "code": code,
                "redeemed_at": ts,
                "role_expires_at": expires,
            })
        })
        .collect();

    let pending_rows = sqlx::query_as::<_, (String, String, Option<String>, DateTime<Utc>)>(
        "SELECT b.name, r.guild_id, b.invite_url, r.redeemed_at FROM redemptions r \
         JOIN code_batches b ON b.id = r.batch_id \
         WHERE r.discord_id = $1 AND r.pending = TRUE \
         ORDER BY r.redeemed_at DESC",
    )
    .bind(discord_id)
    .fetch_all(&state.pool)
    .await?;

    let pending_json: Vec<Value> = pending_rows
        .into_iter()
        .map(|(batch, guild_id, invite_url, ts)| {
            json!({
                "batch": batch,
                "guild_id": guild_id,
                "invite_url": invite_url,
                "redeemed_at": ts,
            })
        })
        .collect();

    Ok(json!({
        "logged_in": true,
        "discord_id": discord_id,
        "display_name": display_name,
        "recent_redemptions": recent_json,
        "pending_redemptions": pending_json,
    }))
}

pub async fn my_redemptions(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
) -> Result<Json<Value>, AppError> {
    let (discord_id, _) = get_session(&jar, &state.config.session_secret)?;

    let rows = sqlx::query_as::<_, (String, String, String, DateTime<Utc>)>(
        "SELECT b.name, c.code, r.guild_id, r.redeemed_at FROM redemptions r \
         JOIN codes c ON c.id = r.code_id \
         JOIN code_batches b ON b.id = r.batch_id \
         WHERE r.discord_id = $1 ORDER BY r.redeemed_at DESC",
    )
    .bind(&discord_id)
    .fetch_all(&state.pool)
    .await?;

    let items: Vec<Value> = rows
        .into_iter()
        .map(|(batch, code, guild_id, ts)| {
            json!({"batch": batch, "code": code, "guild_id": guild_id, "redeemed_at": ts})
        })
        .collect();

    Ok(Json(json!({ "redemptions": items })))
}

pub async fn logout(jar: CookieJar) -> Result<(CookieJar, Json<Value>), AppError> {
    let cookie = format!("{SESSION_COOKIE}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0");
    let jar = jar.add(
        axum_extra::extract::cookie::Cookie::parse(cookie)
            .map_err(|e| AppError::Internal(format!("Cookie parse error: {e}")))?,
    );
    Ok((jar, Json(json!({"success": true}))))
}

pub fn render_verify_page(base_url: &str) -> String {
    format!(
        r##"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1, viewport-fit=cover">
<meta name="theme-color" content="#0e1525">
<link rel="icon" type="image/x-icon" href="{base_url}/favicon.ico">
<title>Referral Code Role — Redeem</title>
<style>
*{{margin:0;padding:0;box-sizing:border-box}}
body{{font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',Roboto,sans-serif;background:#0e1525;color:#c9d1d9;min-height:100vh;display:flex;align-items:center;justify-content:center}}
.wrap{{max-width:480px;width:100%;padding:1.5rem;margin:1rem}}
.card{{background:#161b22;border:1px solid #30363d;border-radius:12px;padding:1.75rem;margin-bottom:1.25rem}}
h1{{font-size:1.5rem;color:#e6edf3;margin-bottom:.5rem}}
.subtitle{{color:#8b949e;font-size:.9rem;margin-bottom:1.5rem}}
.btn{{display:inline-flex;align-items:center;justify-content:center;padding:.85rem 1.5rem;border-radius:8px;font-size:.95rem;font-weight:600;border:none;cursor:pointer;text-decoration:none;width:100%;transition:all .15s}}
.btn-discord{{background:#5865F2;color:#fff}}.btn-discord:hover{{background:#4752c4}}
.btn-redeem{{background:#238636;color:#fff;font-size:1.05rem;padding:1rem 1.5rem}}.btn-redeem:hover{{background:#2ea043}}
.btn-logout{{background:#21262d;color:#c9d1d9;border:1px solid #30363d;margin-top:.75rem}}.btn-logout:hover{{background:#30363d}}
.hidden{{display:none}}
.code-input{{width:100%;padding:1rem;font-size:1.4rem;font-family:'SF Mono',Menlo,Consolas,monospace;text-align:center;letter-spacing:.15em;background:#0d1117;border:2px solid #30363d;color:#e6edf3;border-radius:8px;text-transform:uppercase}}
.code-input:focus{{outline:none;border-color:#58a6ff}}
.msg{{padding:.85rem;border-radius:8px;margin-bottom:1rem;font-size:.9rem}}
.msg-error{{background:#3d1f1f;border:1px solid #f85149;color:#f85149}}
.msg-success{{background:#1f3d1f;border:1px solid #3fb950;color:#3fb950}}
.msg-info{{background:#1f2a3d;border:1px solid #58a6ff;color:#58a6ff}}
.spinner{{width:18px;height:18px;border:2px solid #30363d;border-top-color:#58a6ff;border-radius:50%;animation:spin .6s linear infinite;display:inline-block;vertical-align:middle;margin-right:.5rem}}
@keyframes spin{{to{{transform:rotate(360deg)}}}}
.user-row{{display:flex;align-items:center;justify-content:space-between;margin-bottom:1rem}}
.user-row .name{{font-weight:600;color:#e6edf3}}
.recent{{margin-top:1.25rem}}
.recent h3{{font-size:.78rem;color:#8b949e;text-transform:uppercase;letter-spacing:.05em;margin-bottom:.5rem}}
.recent ul{{list-style:none}}
.recent li{{background:#0d1117;border:1px solid #21262d;border-radius:6px;padding:.55rem .75rem;margin-bottom:.4rem;display:flex;justify-content:space-between;font-size:.85rem}}
.recent li .when{{color:#8b949e;font-size:.78rem}}
.recent li .meta{{display:flex;flex-direction:column;align-items:flex-end;gap:.15rem}}
.expires{{color:#f0b429;font-size:.72rem}}
.expired{{color:#f85149;font-size:.72rem;font-weight:600}}
.label{{font-size:.78rem;color:#8b949e;text-transform:uppercase;letter-spacing:.05em;margin-bottom:.4rem}}
.pending-card{{background:#1f2937;border:1px solid #f0b429;border-radius:10px;padding:1rem 1.1rem;margin-bottom:1.25rem}}
.pending-card h2{{font-size:1.05rem;color:#f0b429;margin-bottom:.4rem;display:flex;align-items:center;gap:.45rem}}
.pending-card p{{color:#c9d1d9;font-size:.88rem;margin-bottom:.85rem;line-height:1.4}}
.pending-item{{background:#0d1117;border:1px solid #30363d;border-radius:8px;padding:.7rem .8rem;margin-bottom:.55rem}}
.pending-item .pi-name{{font-weight:600;color:#e6edf3;font-size:.9rem;margin-bottom:.5rem}}
.pending-item .pi-actions{{display:flex;gap:.5rem;flex-wrap:wrap}}
.btn-join{{background:#5865F2;color:#fff;font-size:.85rem;padding:.55rem .9rem;flex:1;min-width:140px}}
.btn-join:hover{{background:#4752c4}}
.btn-check{{background:#21262d;color:#c9d1d9;border:1px solid #30363d;font-size:.85rem;padding:.55rem .9rem;flex:1;min-width:140px}}
.btn-check:hover{{background:#30363d}}
@media (max-width:480px){{
  body{{align-items:flex-start}}
  .wrap{{padding:1rem;margin:.5rem;padding-top:max(1rem,env(safe-area-inset-top,0));padding-bottom:max(1rem,env(safe-area-inset-bottom,0))}}
  .card{{padding:1.25rem;border-radius:10px}}
  h1{{font-size:1.3rem}}
  .subtitle{{font-size:.85rem;margin-bottom:1.1rem}}
  .code-input{{font-size:1.2rem;padding:.85rem;letter-spacing:.1em}}
  .btn-redeem{{font-size:1rem;padding:.95rem 1.5rem}}
  .recent li{{flex-direction:column;align-items:flex-start;gap:.2rem}}
  .recent li .when{{font-size:.72rem}}
}}
</style>
</head>
<body>
<div class="wrap">
<div class="card">
<h1>Redeem your code</h1>
<p class="subtitle">Got a code from a campaign, podcast, or event? Enter it below to unlock your Discord role.</p>

<div id="msg" class="hidden"></div>
<div id="loading"><span class="spinner"></span> Loading...</div>

<div id="login" class="hidden">
<p style="color:#8b949e;font-size:.88rem;margin-bottom:1rem">First, sign in with the Discord account you want the role on.</p>
<a id="login-btn" class="btn btn-discord" href="#">
<svg width="20" height="20" viewBox="0 0 71 55" fill="white" style="margin-right:8px"><path d="M60.1 4.9A58.5 58.5 0 0045.4.2a.2.2 0 00-.2.1 40.8 40.8 0 00-1.8 3.7 54 54 0 00-16.2 0A26.5 26.5 0 0025.4.3a.2.2 0 00-.2-.1A58.4 58.4 0 0010.5 4.9a.2.2 0 00-.1.1C1.5 18 -.9 30.6.3 43a.2.2 0 00.1.2 58.7 58.7 0 0017.7 9 .2.2 0 00.3-.1 42 42 0 003.6-5.9.2.2 0 00-.1-.3 38.6 38.6 0 01-5.5-2.6.2.2 0 010-.4l1.1-.9a.2.2 0 01.2 0 41.9 41.9 0 0035.6 0 .2.2 0 01.2 0l1.1.9a.2.2 0 010 .4c-1.8 1-3.6 1.8-5.5 2.6a.2.2 0 00-.1.3 47.2 47.2 0 003.6 5.9.2.2 0 00.3.1 58.5 58.5 0 0017.7-9 .2.2 0 00.1-.1c1.4-14.3-2.3-26.7-9.7-37.8a.2.2 0 00-.1-.1zM23.7 35.2c-3.3 0-6-3-6-6.6s2.7-6.6 6-6.6 6.1 3 6 6.6c0 3.7-2.7 6.6-6 6.6zm22.2 0c-3.3 0-6-3-6-6.6s2.6-6.6 6-6.6 6 3 6 6.6-2.6 6.6-6 6.6z"/></svg>
Sign in with Discord
</a>
<p style="color:#8b949e;font-size:.78rem;margin-top:.85rem;text-align:center">We only check who you are. We don't post or DM anything.</p>
</div>

<div id="redeem" class="hidden">
<div class="user-row">
<span class="name" id="username"></span>
<a href="#" onclick="doLogout();return false" style="font-size:.8rem;color:#8b949e;text-decoration:none">Not you? Sign out</a>
</div>
<div id="pending-wrap"></div>
<div class="label">Enter your code</div>
<input id="code-input" class="code-input" type="text" placeholder="ABC123" autocomplete="off" maxlength="40" inputmode="text" autocapitalize="characters" spellcheck="false">
<div style="height:.85rem"></div>
<button id="redeem-btn" class="btn btn-redeem" onclick="doRedeem()">Claim my role</button>
<p style="color:#8b949e;font-size:.78rem;margin-top:.85rem;text-align:center">Codes are case-insensitive. Hyphens are ignored.</p>
<div class="recent" id="recent-wrap"></div>
</div>

</div>
</div>

<script>
const BASE = '{base_url}';
const params = new URLSearchParams(window.location.search);
const PRESET_CODE = (params.get('code') || '').toUpperCase().replace(/[^A-Z0-9]/g, '');

function show(id) {{
  ['loading','login','redeem'].forEach(s => document.getElementById(s).classList.add('hidden'));
  document.getElementById(id).classList.remove('hidden');
}}

function showMsg(text, type) {{
  const el = document.getElementById('msg');
  el.className = 'msg msg-' + type;
  el.textContent = text;
  el.classList.remove('hidden');
  if (type === 'success') setTimeout(() => el.classList.add('hidden'), 6000);
}}

async function api(method, path, body) {{
  const opts = {{ method, credentials: 'include', headers: {{'Content-Type':'application/json'}} }};
  if (body) opts.body = JSON.stringify(body);
  const res = await fetch(BASE + path, opts);
  let data = {{}};
  try {{ data = await res.json(); }} catch(e) {{}}
  if (!res.ok) throw new Error(data.error || 'Request failed');
  return data;
}}

document.getElementById('code-input').addEventListener('input', e => {{
  e.target.value = e.target.value.toUpperCase().replace(/[^A-Z0-9-]/g, '');
}});

document.getElementById('login-btn').addEventListener('click', e => {{
  e.preventDefault();
  const dest = PRESET_CODE
    ? BASE + '/verify/login?code=' + encodeURIComponent(PRESET_CODE)
    : BASE + '/verify/login';
  window.location.href = dest;
}});

const ERROR_TEXT = {{
  invalid: 'Hmm, we don\'t recognise that code. Double-check for typos and try again.',
  expired: 'This code campaign has ended — sorry!',
  exhausted: 'This code campaign has hit its redemption limit.',
  already_used: 'Looks like that code has already been claimed by someone else.',
  already_redeemed_by_you: 'You\'ve already redeemed this code — your role is set.',
}};

async function doRedeem() {{
  const raw = document.getElementById('code-input').value.trim();
  if (!raw) {{ showMsg('Type your code into the box first.', 'error'); return; }}
  const btn = document.getElementById('redeem-btn');
  btn.disabled = true; btn.textContent = 'Working...';
  try {{
    const r = await api('POST', '/verify/redeem', {{ code: raw }});
    if (r.success) {{
      const groupBit = r.batch_name ? ' from "' + r.batch_name + '"' : '';
      const note = r.pending
        ? ' You\'re not in the Discord server yet — join it and your role will be assigned automatically.'
        : ' Your role should appear in Discord within a few seconds.';
      const expBit = r.role_expires_at
        ? ' This role lasts until ' + new Date(r.role_expires_at).toLocaleString() + '.'
        : '';
      showMsg('Code accepted' + groupBit + '!' + note + expBit, 'success');
      document.getElementById('code-input').value = '';
      await refreshStatus();
    }} else if (r.error) {{
      showMsg(ERROR_TEXT[r.error] || r.error, 'error');
    }}
  }} catch(e) {{
    showMsg(e.message, 'error');
  }} finally {{
    btn.disabled = false; btn.textContent = 'Claim my role';
  }}
}}

let POLL_TIMER = null;
let POLL_DEADLINE = 0;

function escHtml(s) {{
  return String(s == null ? '' : s).replace(/[&<>"']/g, c => ({{'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}})[c]);
}}

function renderPending(items) {{
  const wrap = document.getElementById('pending-wrap');
  if (!items || !items.length) {{ wrap.innerHTML = ''; return; }}
  let html = '<div class="pending-card"><h2>Waiting for you to join Discord</h2>';
  html += '<p>Your code was accepted, but you\'re not in the server yet. Join, then we\'ll assign your role automatically — usually within a couple of minutes. Click "I\'ve joined" for an instant check.</p>';
  for (const p of items) {{
    html += '<div class="pending-item"><div class="pi-name">' + escHtml(p.batch) + '</div><div class="pi-actions">';
    if (p.invite_url) {{
      html += '<a class="btn btn-join" href="' + escHtml(p.invite_url) + '" target="_blank" rel="noopener noreferrer">Join the Discord server</a>';
    }}
    html += '<button class="btn btn-check" onclick="doRefresh()">I\'ve joined — check now</button>';
    html += '</div></div>';
  }}
  html += '</div>';
  wrap.innerHTML = html;
}}

async function doRefresh() {{
  const btns = document.querySelectorAll('.btn-check');
  btns.forEach(b => {{ b.disabled = true; b.textContent = 'Checking...'; }});
  try {{
    const s = await api('POST', '/verify/refresh');
    applyStatus(s);
    if (!s.pending_redemptions || !s.pending_redemptions.length) {{
      showMsg('All set — your role has been assigned.', 'success');
    }} else {{
      showMsg('Still not seeing you in the server. If you just joined, give it a few more seconds and try again.', 'info');
    }}
  }} catch(e) {{
    showMsg(e.message, 'error');
  }} finally {{
    btns.forEach(b => {{ b.disabled = false; b.textContent = 'I\'ve joined — check now'; }});
  }}
}}

function startPollingIfPending(items) {{
  const hasPending = items && items.length > 0;
  if (!hasPending) {{
    if (POLL_TIMER) {{ clearInterval(POLL_TIMER); POLL_TIMER = null; }}
    return;
  }}
  POLL_DEADLINE = Date.now() + 5 * 60 * 1000;
  if (POLL_TIMER) return;
  POLL_TIMER = setInterval(async () => {{
    if (Date.now() > POLL_DEADLINE) {{ clearInterval(POLL_TIMER); POLL_TIMER = null; return; }}
    try {{
      const s = await api('GET', '/verify/status');
      applyStatus(s);
      if (!s.pending_redemptions || !s.pending_redemptions.length) {{
        clearInterval(POLL_TIMER); POLL_TIMER = null;
      }}
    }} catch(e) {{}}
  }}, 10000);
}}

async function doLogout() {{
  try {{ await api('POST', '/verify/logout'); }} catch(e) {{}}
  window.location.reload();
}}

function fmtTs(t) {{ return new Date(t).toLocaleString(); }}

function expiryLabel(iso) {{
  if (!iso) return '';
  const t = new Date(iso).getTime();
  if (t <= Date.now()) return '<span class="expired">Expired</span>';
  return '<span class="expires">Expires ' + fmtTs(iso) + '</span>';
}}

function renderRecent(items) {{
  const wrap = document.getElementById('recent-wrap');
  if (!items.length) {{ wrap.innerHTML = ''; return; }}
  let html = '<h3>Codes you\'ve already redeemed</h3><ul>';
  for (const r of items) {{
    const exp = expiryLabel(r.role_expires_at);
    html += '<li><span>' + escHtml(r.batch) + ' &middot; <span style="font-family:monospace">' + escHtml(r.code) + '</span></span>'
         + '<span class="meta"><span class="when">' + fmtTs(r.redeemed_at) + '</span>'
         + (exp ? exp : '') + '</span></li>';
  }}
  html += '</ul>';
  wrap.innerHTML = html;
}}

function applyStatus(s) {{
  if (s.logged_in) {{
    document.getElementById('username').textContent = s.display_name || s.discord_id;
    show('redeem');
    if (PRESET_CODE) document.getElementById('code-input').value = PRESET_CODE;
    renderRecent(s.recent_redemptions || []);
    renderPending(s.pending_redemptions || []);
    startPollingIfPending(s.pending_redemptions || []);
  }} else {{
    show('login');
  }}
}}

async function refreshStatus() {{
  const s = await api('GET', '/verify/status');
  applyStatus(s);
}}

refreshStatus().catch(() => show('login'));
</script>
</body>
</html>"##
    )
}
