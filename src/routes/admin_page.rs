use std::sync::Arc;

use axum::extract::State;
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};

use crate::AppState;

pub async fn admin_page(State(state): State<Arc<AppState>>) -> Response {
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        state.admin_html.clone(),
    )
        .into_response()
}

pub fn render_admin_page(base_url: &str) -> String {
    format!(
        r##"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1, viewport-fit=cover">
<meta name="theme-color" content="#0e1525">
<link rel="icon" type="image/x-icon" href="{base_url}/favicon.ico">
<title>Referral Code Role — Admin</title>
<style>
*{{margin:0;padding:0;box-sizing:border-box}}
body{{font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',Roboto,sans-serif;background:#0e1525;color:#c9d1d9;min-height:100vh}}
.wrap{{max-width:1200px;margin:0 auto;padding:1.5rem}}
header{{display:flex;justify-content:space-between;align-items:flex-start;margin-bottom:1.5rem;flex-wrap:wrap;gap:1rem}}
header h1{{font-size:1.6rem;color:#e6edf3}}
header .sub{{color:#8b949e;font-size:.9rem;margin-top:.25rem}}
.stat-row{{display:grid;grid-template-columns:repeat(auto-fit,minmax(160px,1fr));gap:.75rem;margin-bottom:1.5rem}}
.stat{{background:#161b22;border:1px solid #30363d;border-radius:10px;padding:1rem}}
.stat .v{{font-size:1.6rem;font-weight:600;color:#e6edf3}}
.stat .l{{color:#8b949e;font-size:.75rem;text-transform:uppercase;letter-spacing:.05em;margin-top:.25rem}}
.card{{background:#161b22;border:1px solid #30363d;border-radius:12px;padding:1.25rem;margin-bottom:1.25rem}}
.card-head{{display:flex;justify-content:space-between;align-items:center;margin-bottom:1rem}}
.card-head h2{{font-size:1.1rem;color:#e6edf3}}
table{{width:100%;border-collapse:collapse}}
th,td{{padding:.6rem .75rem;text-align:left;border-bottom:1px solid #21262d;font-size:.9rem}}
th{{font-size:.7rem;text-transform:uppercase;color:#8b949e;letter-spacing:.05em}}
.badge{{display:inline-block;padding:.15rem .55rem;border-radius:20px;font-size:.7rem;font-weight:600;background:#21262d;color:#c9d1d9}}
.badge-uc{{background:#1f3d3a;color:#3fb9b6}}
.badge-uu{{background:#1f3d2c;color:#3fb950}}
.badge-su{{background:#3d2f1f;color:#d29922}}
.btn{{display:inline-flex;align-items:center;justify-content:center;padding:.5rem 1rem;border-radius:6px;font-size:.85rem;font-weight:600;border:none;cursor:pointer;text-decoration:none;transition:all .15s;background:#21262d;color:#c9d1d9}}
.btn:hover{{background:#30363d}}
.btn-primary{{background:#238636;color:#fff}}
.btn-primary:hover{{background:#2ea043}}
.btn-danger{{background:#a32f24;color:#fff}}
.btn-danger:hover{{background:#c93c2f}}
.btn-sm{{padding:.3rem .6rem;font-size:.78rem}}
.row-actions{{display:flex;gap:.4rem;flex-wrap:wrap}}
.modal-bg{{position:fixed;inset:0;background:rgba(0,0,0,.7);display:none;align-items:center;justify-content:center;z-index:50;cursor:pointer}}
.modal-bg.open{{display:flex}}
.modal-bg .modal{{cursor:auto}}
.dur-row{{display:grid;grid-template-columns:1fr 7rem;gap:.5rem;align-items:stretch}}
.dur-row input{{width:100%;min-width:0}}
.dur-row select{{width:100%;padding:.55rem .75rem;background:#0d1117;border:1px solid #30363d;color:#e6edf3;border-radius:6px;font-size:.95rem;font-family:inherit;cursor:pointer}}
.dur-row select:focus{{outline:none;border-color:#58a6ff}}
.modal{{background:#161b22;border:1px solid #30363d;border-radius:12px;padding:1.5rem;width:min(560px,92vw);max-height:90vh;overflow:auto}}
.modal h3{{color:#e6edf3;margin-bottom:1rem;font-size:1.15rem}}
.field{{margin-bottom:1rem}}
.field label{{display:block;font-size:.78rem;color:#8b949e;text-transform:uppercase;letter-spacing:.05em;margin-bottom:.3rem}}
.field input,.field select,.field textarea{{width:100%;padding:.55rem .75rem;background:#0d1117;border:1px solid #30363d;color:#e6edf3;border-radius:6px;font-size:.95rem;font-family:inherit}}
.field input:focus,.field select:focus,.field textarea:focus{{outline:none;border-color:#58a6ff}}
.field .help{{font-size:.75rem;color:#8b949e;margin-top:.3rem}}
.modal-actions{{display:flex;gap:.5rem;justify-content:flex-end;margin-top:1.25rem}}
.empty{{color:#8b949e;text-align:center;padding:2rem;font-size:.9rem}}
.code-mono{{font-family:'SF Mono',Menlo,Consolas,monospace;background:#0d1117;padding:.15rem .4rem;border-radius:4px;font-size:.85rem}}
.toast{{position:fixed;top:1rem;right:1rem;background:#161b22;border:1px solid #30363d;border-radius:8px;padding:.75rem 1rem;color:#e6edf3;font-size:.85rem;z-index:100;display:none;box-shadow:0 4px 12px rgba(0,0,0,.4)}}
.toast.error{{border-color:#f85149;color:#f85149}}
.toast.success{{border-color:#3fb950;color:#3fb950}}
.drawer-bg{{position:fixed;inset:0;background:rgba(0,0,0,.5);display:none;z-index:55;cursor:pointer}}
.drawer-bg.open{{display:block}}
.drawer{{position:fixed;top:0;right:0;height:100vh;width:min(640px,95vw);background:#161b22;border-left:1px solid #30363d;padding:1.5rem;overflow:auto;transform:translateX(100%);transition:transform .2s;z-index:60}}
.drawer.open{{transform:translateX(0)}}
.drawer h3{{color:#e6edf3;margin-bottom:1rem}}
.drawer-head{{display:flex;justify-content:space-between;align-items:center;margin-bottom:1rem}}
.tab-row{{display:flex;gap:.4rem;margin-bottom:1rem;border-bottom:1px solid #30363d}}
.tab{{padding:.5rem .75rem;cursor:pointer;color:#8b949e;border-bottom:2px solid transparent}}
.tab.active{{color:#e6edf3;border-bottom-color:#58a6ff}}
.muted{{color:#8b949e;font-size:.8rem}}
.qr-preview{{background:#fff;padding:.5rem;border-radius:8px;display:inline-block;margin-top:.5rem}}
.qr-preview svg{{display:block;max-width:200px;height:auto}}
.preview-list{{background:#0d1117;border:1px solid #30363d;border-radius:6px;padding:.75rem;max-height:200px;overflow:auto;font-family:'SF Mono',Menlo,monospace;font-size:.85rem}}
.hidden{{display:none}}
.gate{{max-width:480px;margin:4rem auto;background:#161b22;border:1px solid #30363d;border-radius:12px;padding:1.75rem;text-align:center}}
.gate h1{{font-size:1.4rem;color:#e6edf3;margin-bottom:.5rem}}
.gate p{{color:#8b949e;font-size:.9rem;margin-bottom:1.25rem}}
.gate .btn-discord{{background:#5865F2;color:#fff;width:100%;padding:.85rem 1.5rem;font-weight:600;border-radius:8px;text-decoration:none;display:inline-flex;align-items:center;justify-content:center}}
.gate .btn-discord:hover{{background:#4752c4}}
.gate.error{{border-color:#f85149}}
.gate.error h1{{color:#f85149}}
.tbl-wrap{{overflow-x:auto;-webkit-overflow-scrolling:touch;margin:0 -.25rem}}
.tbl-wrap table{{min-width:520px}}
.welcome{{background:linear-gradient(135deg,#1a2942 0%,#161b22 100%);border:1px solid #30363d;border-left:3px solid #58a6ff;border-radius:12px;padding:1.4rem;margin-bottom:1.25rem}}
.welcome h2{{color:#e6edf3;font-size:1.15rem;margin-bottom:.25rem}}
.welcome p{{color:#8b949e;font-size:.9rem;margin-bottom:1rem}}
.welcome ol{{margin:0 0 1rem 1.1rem;color:#c9d1d9;font-size:.9rem;line-height:1.6}}
.welcome ol li{{margin-bottom:.25rem}}
.welcome ol li strong{{color:#e6edf3}}
.welcome .actions{{display:flex;gap:.5rem;flex-wrap:wrap}}
.kind-picker{{display:grid;grid-template-columns:1fr;gap:.55rem}}
.kind-card{{background:#0d1117;border:1px solid #30363d;border-radius:8px;padding:.85rem;cursor:pointer;transition:all .12s}}
.kind-card:hover{{border-color:#484f58}}
.kind-card.selected{{border-color:#58a6ff;background:#10243a}}
.kind-card .k-title{{color:#e6edf3;font-weight:600;font-size:.95rem;margin-bottom:.2rem;display:flex;align-items:center;gap:.5rem}}
.kind-card .k-eg{{color:#8b949e;font-size:.78rem;margin-bottom:.3rem}}
.kind-card .k-desc{{color:#c9d1d9;font-size:.82rem;line-height:1.45}}
.url-row{{display:flex;align-items:center;gap:.4rem;background:#0d1117;border:1px solid #30363d;border-radius:6px;padding:.4rem .55rem;margin-top:.4rem;font-size:.8rem}}
.url-row code{{flex:1;font-family:'SF Mono',Menlo,monospace;color:#58a6ff;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}}
.tip{{background:#1a2330;border-left:3px solid #58a6ff;border-radius:6px;padding:.65rem .85rem;margin-bottom:.85rem;color:#c9d1d9;font-size:.85rem;line-height:1.5}}
.tip strong{{color:#e6edf3}}
.share-card{{background:#0d1117;border:1px solid #30363d;border-radius:10px;padding:1rem;margin-bottom:1rem}}
.share-card .label{{color:#8b949e;font-size:.72rem;text-transform:uppercase;letter-spacing:.05em;margin-bottom:.4rem}}
@media (max-width:640px){{
  .wrap{{padding:1rem .85rem;padding-bottom:env(safe-area-inset-bottom,1rem)}}
  header{{flex-direction:column;align-items:stretch;gap:.85rem;margin-bottom:1.1rem}}
  header h1{{font-size:1.35rem}}
  header .sub{{font-size:.82rem}}
  header .btn-primary{{width:100%;padding:.7rem 1rem}}
  .stat-row{{grid-template-columns:repeat(2,1fr);gap:.5rem;margin-bottom:1.1rem}}
  .stat{{padding:.75rem .85rem}}
  .stat .v{{font-size:1.25rem}}
  .stat .l{{font-size:.68rem}}
  .card{{padding:1rem .85rem;margin-bottom:1rem;border-radius:10px}}
  .card-head h2{{font-size:1rem}}
  th,td{{padding:.5rem .55rem;font-size:.82rem}}
  th{{font-size:.65rem}}
  .btn{{padding:.55rem .9rem;font-size:.85rem}}
  .btn-sm{{padding:.4rem .65rem;font-size:.78rem}}
  .row-actions{{gap:.3rem}}
  .row-actions .btn{{flex:1 1 auto;min-width:0}}
  .modal{{width:100vw;max-width:100vw;height:100vh;max-height:100vh;border-radius:0;padding:1.1rem;padding-top:max(1.1rem,env(safe-area-inset-top,0));padding-bottom:max(1.1rem,env(safe-area-inset-bottom,0))}}
  .modal-bg{{align-items:stretch;justify-content:stretch}}
  .modal-actions{{position:sticky;bottom:0;background:#161b22;padding-top:.75rem;margin-top:1rem;justify-content:stretch}}
  .modal-actions .btn{{flex:1 1 0}}
  .drawer{{width:100vw;padding:1rem;padding-top:max(1rem,env(safe-area-inset-top,0));padding-bottom:max(1rem,env(safe-area-inset-bottom,0))}}
  .drawer h3{{font-size:1.05rem}}
  .gate{{margin:1.5rem .85rem;padding:1.4rem 1.1rem}}
  .gate h1{{font-size:1.2rem}}
  .toast{{top:auto;bottom:1rem;left:1rem;right:1rem;text-align:center}}
}}
</style>
</head>
<body>
<div id="boot" class="gate"><h1>Loading...</h1><p>Checking your session.</p></div>

<div id="auth-gate" class="gate hidden">
<h1>Sign in to manage codes</h1>
<p>Sign in with the Discord account that has <strong>Manage Server</strong> permission. We use it to confirm you're an admin of this guild.</p>
<a id="login-btn" class="btn-discord" href="#">
<svg width="20" height="20" viewBox="0 0 71 55" fill="white" style="margin-right:8px"><path d="M60.1 4.9A58.5 58.5 0 0045.4.2a.2.2 0 00-.2.1 40.8 40.8 0 00-1.8 3.7 54 54 0 00-16.2 0A26.5 26.5 0 0025.4.3a.2.2 0 00-.2-.1A58.4 58.4 0 0010.5 4.9a.2.2 0 00-.1.1C1.5 18 -.9 30.6.3 43a.2.2 0 00.1.2 58.7 58.7 0 0017.7 9 .2.2 0 00.3-.1 42 42 0 003.6-5.9.2.2 0 00-.1-.3 38.6 38.6 0 01-5.5-2.6.2.2 0 010-.4l1.1-.9a.2.2 0 01.2 0 41.9 41.9 0 0035.6 0 .2.2 0 01.2 0l1.1.9a.2.2 0 010 .4c-1.8 1-3.6 1.8-5.5 2.6a.2.2 0 00-.1.3 47.2 47.2 0 003.6 5.9.2.2 0 00.3.1 58.5 58.5 0 0017.7-9 .2.2 0 00.1-.1c1.4-14.3-2.3-26.7-9.7-37.8a.2.2 0 00-.1-.1zM23.7 35.2c-3.3 0-6-3-6-6.6s2.7-6.6 6-6.6 6.1 3 6 6.6c0 3.7-2.7 6.6-6 6.6zm22.2 0c-3.3 0-6-3-6-6.6s2.6-6.6 6-6.6 6 3 6 6.6-2.6 6.6-6 6.6z"/></svg>
Sign in with Discord
</a>
</div>

<div id="error-gate" class="gate error hidden">
<h1 id="error-title">Access denied</h1>
<p id="error-msg"></p>
<a id="error-action" class="btn-discord" href="#" style="margin-top:.5rem">Switch account</a>
</div>

<div id="admin-shell" class="wrap hidden">
<header>
<div>
<h1>Referral Code Role</h1>
<div class="sub"><span id="guild-name"></span> &middot; <span id="admin-name" class="muted"></span> &middot; <a href="#" onclick="doLogout();return false" style="color:#8b949e;text-decoration:none">Sign out</a></div>
</div>
<button class="btn btn-primary" onclick="openCreateBatch()">+ New code group</button>
</header>

<div id="welcome-banner"></div>

<div class="stat-row" id="stats"></div>

<div class="share-card" id="share-card">
<div class="label">Redemption page (share with members)</div>
<div class="url-row"><code id="share-url"></code><button class="btn btn-sm" onclick="copyShareUrl()">Copy</button><a class="btn btn-sm" id="share-open" target="_blank" rel="noopener">Open</a></div>
</div>

<div class="card">
<div class="card-head"><h2>Code groups</h2></div>
<div id="batch-table-wrap"><div class="empty">Loading...</div></div>
</div>
</div>

<div class="modal-bg" id="m-create">
<div class="modal">
<h3>New code group</h3>
<div class="tip"><strong>What's a code group?</strong> A bundle of codes that share the same rules — for example, "Kickstarter Tier 2" might contain 200 unique codes, while "Podcast Episode 47" might be one shared code. You'll generate codes inside the group after creating it.</div>

<div class="field"><label>Group name</label><input id="b-name" type="text" placeholder="e.g. Kickstarter Tier 2, Podcast Ep. 47, Comic-Con 2026">
<div class="help">Just a label so you can find it later.</div></div>

<div class="field"><label>Notes (optional)</label><input id="b-desc" type="text" placeholder="Anything you want to remember about this group"></div>

<div class="field"><label>How will codes be used?</label>
<input type="hidden" id="b-kind" value="unique_per_code">
<div class="kind-picker" id="kind-picker">
<div class="kind-card selected" data-kind="unique_per_code" onclick="pickKind('unique_per_code')">
<div class="k-title">One-time codes</div>
<div class="k-eg">Best for: Kickstarter rewards, NFT drops, raffle prizes</div>
<div class="k-desc">Each code can be redeemed by exactly one person, then burns out. Generate as many codes as you have rewards.</div>
</div>
<div class="kind-card" data-kind="unique_per_user" onclick="pickKind('unique_per_user')">
<div class="k-title">Per-person codes</div>
<div class="k-eg">Best for: event wristbands, conference badges, IRL meetups</div>
<div class="k-desc">A code can be on many wristbands, and each person can claim once across the whole group. Stops one person from grabbing the role twice.</div>
</div>
<div class="kind-card" data-kind="shared_unlimited" onclick="pickKind('shared_unlimited')">
<div class="k-title">Single shared code</div>
<div class="k-eg">Best for: podcast shout-outs, livestream giveaways, broadcasts</div>
<div class="k-desc">One code (e.g. PODCODE) that anyone can redeem. Set a redemption cap below if you want a hard limit.</div>
</div>
</div>
</div>

<div class="field"><label>Stop accepting after total redemptions (optional)</label><input id="b-mrt" type="number" min="1" placeholder="e.g. 100">
<div class="help">Leave blank for no limit.</div></div>

<div class="field"><label>Expires (optional)</label><input id="b-exp" type="datetime-local">
<div class="help">After this date, codes in the group stop working. Leave blank to never expire.</div></div>

<div class="field"><label>Discord invite URL (optional)</label><input id="b-invite" type="url" placeholder="https://discord.gg/your-invite">
<div class="help">Shown to people who redeem a code <em>before</em> joining your server, so they can join in one click. Use a non-expiring invite if possible.</div></div>

<div class="field"><label>How long should the role last?</label>
<div class="dur-row">
<input id="b-dur-amount" type="number" min="0" placeholder="e.g. 7">
<select id="b-dur-unit">
<option value="hours">hours</option>
<option value="days" selected>days</option>
<option value="weeks">weeks</option>
</select>
</div>
<div class="help">Leave blank for a <strong>permanent</strong> role. Otherwise the role is automatically removed this long after each redemption (e.g. a 24-hour event pass).</div></div>

<input type="hidden" id="b-mrpc">

<div class="modal-actions">
<button class="btn" onclick="closeModal('m-create')">Cancel</button>
<button class="btn btn-primary" onclick="submitCreateBatch()">Create group</button>
</div>
</div>
</div>

<div class="modal-bg" id="m-gen">
<div class="modal">
<h3>Generate codes</h3>
<div id="g-tip" class="tip"></div>

<div class="field"><label>How many codes?</label><input id="g-count" type="number" min="1" max="1000" value="10">
<div class="help">Up to 1000 at a time.</div></div>

<div class="field"><label>Code length</label><input id="g-length" type="number" min="6" max="20" value="12">
<div class="help">Longer = harder to guess. 12 is a good default.</div></div>

<div class="field"><label>Prefix (optional)</label><input id="g-prefix" type="text" placeholder="KS-">
<div class="help">Codes will start with this — e.g. <code>KS-A1B2C3D4E5F6</code>. Helps you tell groups apart at a glance.</div></div>

<div class="field"><label>Or — paste your own codes</label><textarea id="g-custom" rows="4" placeholder="PODCODE&#10;LIVESTREAM2026&#10;HALLOWEEN"></textarea>
<div class="help">One per line. If you fill this in, the settings above are ignored. Letters and digits only — case is normalised.</div></div>

<div class="modal-actions">
<button class="btn" onclick="closeModal('m-gen')">Cancel</button>
<button class="btn btn-primary" onclick="submitGenerate()">Generate codes</button>
</div>
</div>
</div>

<div class="drawer-bg" id="d-codes-bg"></div>
<div class="drawer" id="d-codes">
<div class="drawer-head"><h3 id="d-codes-title">Codes</h3>
<div><button class="btn btn-sm btn-primary" onclick="openGenerateForCurrent()">+ Generate</button>
<button class="btn btn-sm" onclick="closeDrawer('d-codes')" style="margin-left:.4rem">Close</button></div></div>
<div class="tab-row">
<div class="tab active" id="tab-codes" onclick="switchTab('codes')">Codes</div>
<div class="tab" id="tab-redeems" onclick="switchTab('redeems')">Who redeemed</div>
</div>
<div id="codes-pane"><div class="empty">Loading...</div></div>
<div id="redeems-pane" style="display:none"><div class="empty">Loading...</div></div>
</div>

<div class="toast" id="toast"></div>

<script>
const BASE = '{base_url}';
const params = new URLSearchParams(window.location.search);
const GUILD_ID = params.get('guild_id') || '';
let CURRENT_BATCH = null;

function loginUrl() {{
  const ret = '/referral-code-role/admin' + (GUILD_ID ? '?guild_id=' + encodeURIComponent(GUILD_ID) : '');
  return '/auth/login?return_to=' + encodeURIComponent(ret);
}}

function showPanel(id) {{
  ['boot','auth-gate','error-gate','admin-shell'].forEach(p => {{
    document.getElementById(p).classList.toggle('hidden', p !== id);
  }});
}}

function showAuthGate() {{
  document.getElementById('login-btn').href = loginUrl();
  showPanel('auth-gate');
}}

function showError(title, msg, actionLabel) {{
  document.getElementById('error-title').textContent = title;
  document.getElementById('error-msg').textContent = msg;
  const a = document.getElementById('error-action');
  a.textContent = actionLabel || 'Switch account';
  a.href = loginUrl();
  a.onclick = async (e) => {{
    e.preventDefault();
    try {{ await fetch(BASE + '/verify/logout', {{method:'POST', credentials:'include'}}); }} catch(_) {{}}
    window.location.href = loginUrl();
  }};
  showPanel('error-gate');
}}

async function doLogout() {{
  try {{ await fetch(BASE + '/verify/logout', {{method:'POST', credentials:'include'}}); }} catch(_) {{}}
  window.location.reload();
}}

function toast(msg, kind) {{
  const el = document.getElementById('toast');
  el.className = 'toast ' + (kind || '');
  el.textContent = msg;
  el.style.display = 'block';
  setTimeout(() => el.style.display = 'none', 3500);
}}

async function api(method, path, body) {{
  const opts = {{ method, credentials: 'include', headers: {{'Content-Type':'application/json'}} }};
  if (body !== undefined) opts.body = JSON.stringify(body);
  const sep = path.includes('?') ? '&' : '?';
  const res = await fetch(BASE + path + sep + 'guild_id=' + encodeURIComponent(GUILD_ID), opts);
  if (!res.ok) {{
    let err = 'Request failed';
    try {{ const d = await res.json(); err = d.error || err; }} catch(e) {{}}
    throw new Error(err);
  }}
  const ct = res.headers.get('content-type') || '';
  if (ct.includes('application/json')) return res.json();
  return res.text();
}}

function openModal(id) {{ document.getElementById(id).classList.add('open'); }}
function closeModal(id) {{ document.getElementById(id).classList.remove('open'); }}
function openDrawer(id) {{
  document.getElementById(id).classList.add('open');
  const bg = document.getElementById(id + '-bg');
  if (bg) bg.classList.add('open');
}}
function closeDrawer(id) {{
  document.getElementById(id).classList.remove('open');
  const bg = document.getElementById(id + '-bg');
  if (bg) bg.classList.remove('open');
}}

document.addEventListener('click', (e) => {{
  const mb = e.target.closest('.modal-bg');
  if (mb && e.target === mb) mb.classList.remove('open');
  const db = e.target.closest('.drawer-bg');
  if (db && e.target === db) {{
    db.classList.remove('open');
    const drawerId = db.id.replace(/-bg$/, '');
    const dr = document.getElementById(drawerId);
    if (dr) dr.classList.remove('open');
  }}
}});

document.addEventListener('keydown', (e) => {{
  if (e.key !== 'Escape') return;
  const mb = document.querySelector('.modal-bg.open');
  if (mb) {{ mb.classList.remove('open'); return; }}
  const db = document.querySelector('.drawer-bg.open');
  if (db) {{
    db.classList.remove('open');
    const drawerId = db.id.replace(/-bg$/, '');
    const dr = document.getElementById(drawerId);
    if (dr) dr.classList.remove('open');
  }}
}});

const KIND_LABEL = {{
  unique_per_code: 'One-time codes',
  unique_per_user: 'Per-person codes',
  shared_unlimited: 'Single shared code'
}};
const GEN_TIP = {{
  unique_per_code: '<strong>One-time codes:</strong> generate one code per reward you want to hand out. Each code burns out after a single redemption.',
  unique_per_user: '<strong>Per-person codes:</strong> a single code can be printed on many wristbands. Each Discord user can only redeem once across the whole group.',
  shared_unlimited: '<strong>Single shared code:</strong> usually you want just one (e.g. PODCODE). Use the "paste your own codes" box below to set a memorable word.'
}};

function updateKindHelp() {{ /* legacy no-op kept for compatibility */ }}

function pickKind(k) {{
  document.getElementById('b-kind').value = k;
  document.querySelectorAll('#kind-picker .kind-card').forEach(el => {{
    el.classList.toggle('selected', el.dataset.kind === k);
  }});
}}

function kindBadge(k) {{
  const cls = k === 'unique_per_code' ? 'badge-uc' : k === 'unique_per_user' ? 'badge-uu' : 'badge-su';
  return '<span class="badge ' + cls + '">' + (KIND_LABEL[k] || k) + '</span>';
}}

function copyShareUrl() {{
  const url = document.getElementById('share-url').textContent;
  if (!url) return;
  navigator.clipboard.writeText(url).then(
    () => toast('Redemption page URL copied', 'success'),
    () => toast('Copy failed', 'error')
  );
}}

function renderWelcomeBanner(numBatches) {{
  const el = document.getElementById('welcome-banner');
  if (numBatches > 0) {{ el.innerHTML = ''; return; }}
  el.innerHTML = '<div class="welcome">'
    + '<h2>Welcome! Let\'s set up your first code group.</h2>'
    + '<p>Three quick steps and you\'ll be handing out roles.</p>'
    + '<ol>'
    + '<li><strong>Create a code group</strong> — name it after the campaign (e.g. "Kickstarter Tier 2").</li>'
    + '<li><strong>Generate codes</strong> — one click makes 10, 100, or whatever you need. Each code gets a copy-paste URL and a downloadable QR.</li>'
    + '<li><strong>Approve the group in RoleLogic</strong> — go back to the role config and tick this group. Then anyone who redeems gets the role automatically.</li>'
    + '</ol>'
    + '<div class="actions"><button class="btn btn-primary" onclick="openCreateBatch()">+ Create your first group</button></div>'
    + '</div>';
}}

function fmtTs(ts) {{ return ts ? new Date(ts).toLocaleString() : '-'; }}

function formatDuration(hours) {{
  if (!hours || hours <= 0) return '<span class="muted">Permanent</span>';
  if (hours % (24 * 7) === 0) {{ const w = hours / (24 * 7); return w + (w === 1 ? ' week' : ' weeks'); }}
  if (hours % 24 === 0) {{ const d = hours / 24; return d + (d === 1 ? ' day' : ' days'); }}
  return hours + (hours === 1 ? ' hour' : ' hours');
}}

async function loadStats() {{
  try {{
    const s = await api('GET', '/admin/api/stats');
    document.getElementById('stats').innerHTML = [
      ['Code groups', s.batches], ['Codes', s.codes],
      ['Total redeemed', s.redemptions], ['Redeemed today', s.redemptions_last_24h]
    ].map(([l,v]) => '<div class="stat"><div class="v">' + v + '</div><div class="l">' + l + '</div></div>').join('');
  }} catch(e) {{ toast(e.message, 'error'); }}
}}

async function loadBatches() {{
  try {{
    const r = await api('GET', '/admin/api/batches');
    const wrap = document.getElementById('batch-table-wrap');
    renderWelcomeBanner(r.batches.length);
    if (!r.batches.length) {{
      wrap.innerHTML = '<div class="empty">No code groups yet. Click <strong>+ New code group</strong> above to create one.</div>';
      return;
    }}
    let html = '<div class="tbl-wrap"><table><thead><tr><th>Name</th><th>Type</th><th>Role</th><th>Codes</th><th>Redeemed</th><th>Expires</th><th></th></tr></thead><tbody>';
    for (const b of r.batches) {{
      const pct = b.total_codes > 0 ? Math.round((b.total_redemptions / Math.max(b.total_codes, 1)) * 100) : 0;
      const redeemedText = b.kind === 'shared_unlimited'
        ? b.total_redemptions
        : b.total_redemptions + ' of ' + b.total_codes + ' (' + pct + '%)';
      html += '<tr>'
        + '<td><strong>' + escapeHtml(b.name) + '</strong>' + (b.description ? '<div class="muted">' + escapeHtml(b.description) + '</div>' : '') + '</td>'
        + '<td>' + kindBadge(b.kind) + '</td>'
        + '<td>' + formatDuration(b.role_duration_hours) + '</td>'
        + '<td>' + b.total_codes + '</td>'
        + '<td>' + redeemedText + '</td>'
        + '<td>' + fmtTs(b.expires_at) + '</td>'
        + '<td><div class="row-actions">'
        + '<button class="btn btn-sm btn-primary" onclick="openCodes(' + b.id + ', \'' + escapeAttr(b.name) + '\', \'' + b.kind + '\')">Manage codes</button>'
        + '<button class="btn btn-sm" onclick="openGenerate(' + b.id + ', \'' + escapeAttr(b.name) + '\', \'' + b.kind + '\')">+ More codes</button>'
        + '<button class="btn btn-sm btn-danger" onclick="revokeBatch(' + b.id + ', \'' + escapeAttr(b.name) + '\')">Retire</button>'
        + '</div></td></tr>';
    }}
    html += '</tbody></table></div>';
    wrap.innerHTML = html;
  }} catch(e) {{ toast(e.message, 'error'); }}
}}

function escapeHtml(s) {{ return String(s ?? '').replace(/[&<>"]/g, c => ({{'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;'}})[c]); }}
function escapeAttr(s) {{ return String(s ?? '').replace(/['\\\\]/g, c => '\\\\' + c); }}

function openCreateBatch() {{
  ['b-name','b-desc','b-mrpc','b-mrt','b-exp','b-invite','b-dur-amount'].forEach(i => document.getElementById(i).value = '');
  document.getElementById('b-dur-unit').value = 'days';
  pickKind('unique_per_code');
  openModal('m-create');
  setTimeout(() => document.getElementById('b-name').focus(), 50);
}}

function readDurationHours() {{
  const raw = document.getElementById('b-dur-amount').value;
  if (!raw) return null;
  const n = parseInt(raw, 10);
  if (!Number.isFinite(n) || n <= 0) return null;
  const unit = document.getElementById('b-dur-unit').value;
  const mult = unit === 'days' ? 24 : unit === 'weeks' ? 24 * 7 : 1;
  return n * mult;
}}

async function submitCreateBatch() {{
  const body = {{
    name: document.getElementById('b-name').value.trim(),
    description: document.getElementById('b-desc').value.trim() || null,
    kind: document.getElementById('b-kind').value,
    max_redemptions_per_code: parseIntOrNull('b-mrpc'),
    max_redemptions_total: parseIntOrNull('b-mrt'),
    expires_at: parseDateOrNull('b-exp'),
    invite_url: document.getElementById('b-invite').value.trim() || null,
    role_duration_hours: readDurationHours(),
  }};
  if (!body.name) {{ toast('Give the group a name first', 'error'); return; }}
  try {{
    const r = await api('POST', '/admin/api/batches', body);
    closeModal('m-create');
    toast('Group created — now generate some codes', 'success');
    await loadBatches(); await loadStats();
    if (r && r.id) openGenerate(r.id, body.name, body.kind);
  }} catch(e) {{ toast(e.message, 'error'); }}
}}

function parseIntOrNull(id) {{ const v = document.getElementById(id).value; return v ? parseInt(v, 10) : null; }}
function parseDateOrNull(id) {{ const v = document.getElementById(id).value; return v ? new Date(v).toISOString() : null; }}

function openGenerate(batchId, name, kind) {{
  CURRENT_BATCH = {{ id: batchId, name, kind: kind || (CURRENT_BATCH && CURRENT_BATCH.kind) || 'unique_per_code' }};
  ['g-count','g-length','g-prefix','g-custom'].forEach(i => document.getElementById(i).value = '');
  if (CURRENT_BATCH.kind === 'shared_unlimited') {{
    document.getElementById('g-count').value = '1';
    document.getElementById('g-custom').value = '';
  }} else {{
    document.getElementById('g-count').value = '10';
  }}
  document.getElementById('g-length').value = '12';
  document.getElementById('g-tip').innerHTML = GEN_TIP[CURRENT_BATCH.kind] || '';
  openModal('m-gen');
}}

function openGenerateForCurrent() {{
  if (!CURRENT_BATCH) return;
  closeDrawer('d-codes');
  openGenerate(CURRENT_BATCH.id, CURRENT_BATCH.name, CURRENT_BATCH.kind);
}}

async function submitGenerate() {{
  if (!CURRENT_BATCH) return;
  const customRaw = document.getElementById('g-custom').value.trim();
  const body = {{}};
  if (customRaw) {{
    body.custom_codes = customRaw.split(/\\r?\\n/).map(s => s.trim()).filter(Boolean);
  }} else {{
    body.count = parseInt(document.getElementById('g-count').value, 10) || 0;
    body.length = parseInt(document.getElementById('g-length').value, 10) || 12;
    const p = document.getElementById('g-prefix').value.trim();
    if (p) body.prefix = p;
  }}
  try {{
    const r = await api('POST', '/admin/api/batches/' + CURRENT_BATCH.id + '/codes', body);
    closeModal('m-gen');
    toast(r.inserted + ' code' + (r.inserted === 1 ? '' : 's') + ' added', 'success');
    await loadBatches(); await loadStats();
    openCodes(CURRENT_BATCH.id, CURRENT_BATCH.name, CURRENT_BATCH.kind);
  }} catch(e) {{ toast(e.message, 'error'); }}
}}

async function revokeBatch(id, name) {{
  if (!confirm('Retire the group "' + name + '"? People who already redeemed keep their role, but no new codes from this group will work.')) return;
  try {{
    await api('DELETE', '/admin/api/batches/' + id);
    toast('Group retired', 'success');
    await loadBatches(); await loadStats();
  }} catch(e) {{ toast(e.message, 'error'); }}
}}

async function openCodes(batchId, name, kind) {{
  CURRENT_BATCH = {{ id: batchId, name, kind: kind || 'unique_per_code' }};
  document.getElementById('d-codes-title').textContent = name;
  switchTab('codes');
  openDrawer('d-codes');
  await loadCodes();
}}

function switchTab(which) {{
  document.getElementById('tab-codes').classList.toggle('active', which === 'codes');
  document.getElementById('tab-redeems').classList.toggle('active', which === 'redeems');
  document.getElementById('codes-pane').style.display = which === 'codes' ? '' : 'none';
  document.getElementById('redeems-pane').style.display = which === 'redeems' ? '' : 'none';
  if (which === 'redeems') loadRedemptions();
}}

async function loadCodes() {{
  try {{
    const r = await api('GET', '/admin/api/batches/' + CURRENT_BATCH.id + '/codes');
    const pane = document.getElementById('codes-pane');
    if (!r.codes.length) {{
      pane.innerHTML = '<div class="empty">No codes in this group yet. Click <strong>+ Generate</strong> at the top to add some.</div>';
      return;
    }}
    const tip = '<div class="tip"><strong>Hand these out:</strong> click <em>Copy URL</em> to copy a clickable redemption link, or <em>QR</em> to download a printable QR code (great for stickers, posters, wristbands).</div>';
    let html = tip + '<div class="tbl-wrap"><table><thead><tr><th>Code</th><th>Used</th><th>Status</th><th></th></tr></thead><tbody>';
    for (const c of r.codes) {{
      html += '<tr><td><span class="code-mono">' + escapeHtml(c.code) + '</span></td>'
        + '<td>' + c.uses_count + '×</td>'
        + '<td>' + (c.revoked_at ? '<span class="badge">retired</span>' : '<span class="badge badge-uu">active</span>') + '</td>'
        + '<td><div class="row-actions">'
        + '<button class="btn btn-sm" onclick="copyCodeUrl(\'' + escapeAttr(c.code) + '\')">Copy URL</button>'
        + '<button class="btn btn-sm" onclick="downloadQr(' + c.id + ', \'' + escapeAttr(c.code) + '\')">QR</button>'
        + (c.revoked_at ? '' : '<button class="btn btn-sm btn-danger" onclick="revokeCode(' + c.id + ')">Retire</button>')
        + '</div></td></tr>';
    }}
    html += '</tbody></table></div>';
    pane.innerHTML = html;
  }} catch(e) {{ toast(e.message, 'error'); }}
}}

async function loadRedemptions() {{
  try {{
    const r = await api('GET', '/admin/api/batches/' + CURRENT_BATCH.id + '/redemptions');
    const pane = document.getElementById('redeems-pane');
    if (!r.redemptions.length) {{
      pane.innerHTML = '<div class="empty">Nobody has redeemed a code from this group yet.</div>';
      return;
    }}
    let html = '<div class="tbl-wrap"><table><thead><tr><th>When</th><th>Discord ID</th><th>Code used</th></tr></thead><tbody>';
    for (const x of r.redemptions) {{
      html += '<tr><td>' + fmtTs(x.redeemed_at) + '</td>'
        + '<td><span class="code-mono">' + escapeHtml(x.discord_id) + '</span></td>'
        + '<td><span class="code-mono">' + escapeHtml(x.code) + '</span></td></tr>';
    }}
    html += '</tbody></table></div>';
    pane.innerHTML = html;
  }} catch(e) {{ toast(e.message, 'error'); }}
}}

function codeRedeemUrl(code) {{
  return BASE + '/verify?code=' + encodeURIComponent(code);
}}

function copyCodeUrl(code) {{
  navigator.clipboard.writeText(codeRedeemUrl(code)).then(
    () => toast('Redemption URL copied', 'success'),
    () => toast('Copy failed', 'error')
  );
}}

async function downloadQr(id, code) {{
  try {{
    const url = BASE + '/admin/api/codes/' + id + '/qr.svg?guild_id=' + encodeURIComponent(GUILD_ID);
    const res = await fetch(url, {{credentials:'include'}});
    if (!res.ok) throw new Error('QR fetch failed');
    const blob = await res.blob();
    const objectUrl = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = objectUrl;
    a.download = 'qr-' + code + '.svg';
    document.body.appendChild(a);
    a.click();
    a.remove();
    setTimeout(() => URL.revokeObjectURL(objectUrl), 1000);
  }} catch(e) {{ toast(e.message, 'error'); }}
}}

async function revokeCode(id) {{
  if (!confirm('Retire this code? It will stop working immediately. People who already redeemed it keep their role.')) return;
  try {{
    await api('DELETE', '/admin/api/codes/' + id);
    toast('Code retired', 'success');
    await loadCodes();
  }} catch(e) {{ toast(e.message, 'error'); }}
}}

async function init() {{
  if (!GUILD_ID) {{
    showError('Open this from RoleLogic', 'This admin page needs to know which Discord server you\'re managing. Open it from your RoleLogic dashboard (the Referral Code Role plugin link), which sends the right info.', 'Reload');
    return;
  }}
  let s;
  try {{
    const res = await fetch(BASE + '/verify/status', {{credentials:'include'}});
    s = await res.json();
  }} catch(e) {{
    showError('Network error', 'Could not reach the server. Check your connection and try again.', 'Retry');
    document.getElementById('error-action').onclick = (e) => {{ e.preventDefault(); window.location.reload(); }};
    return;
  }}
  if (!s.logged_in) {{ showAuthGate(); return; }}

  document.getElementById('admin-name').textContent = s.display_name || s.discord_id;

  try {{
    const res = await fetch(BASE + '/admin/api/stats?guild_id=' + encodeURIComponent(GUILD_ID), {{credentials:'include'}});
    if (res.status === 401) {{ showAuthGate(); return; }}
    if (res.status === 403) {{
      showError('You\'re not a manager here', 'You\'re signed in as ' + (s.display_name || s.discord_id) + ', but that account doesn\'t have Manage Server permission in this guild. Either sign in with a different account, or ask a server admin to give you the permission.');
      return;
    }}
    if (!res.ok) {{
      let err = 'Failed to load admin data';
      try {{ const d = await res.json(); err = d.error || err; }} catch(_) {{}}
      showError('Error', err, 'Reload');
      document.getElementById('error-action').onclick = (e) => {{ e.preventDefault(); window.location.reload(); }};
      return;
    }}
    const stats = await res.json();
    document.getElementById('guild-name').textContent = stats.guild_name || 'Guild ' + GUILD_ID;
    document.getElementById('stats').innerHTML = [
      ['Batches', stats.batches], ['Codes', stats.codes],
      ['Redemptions', stats.redemptions], ['Last 24h', stats.redemptions_last_24h]
    ].map(([l,v]) => '<div class="stat"><div class="v">' + v + '</div><div class="l">' + l + '</div></div>').join('');
  }} catch(e) {{
    showError('Error', e.message, 'Reload');
    document.getElementById('error-action').onclick = (e) => {{ e.preventDefault(); window.location.reload(); }};
    return;
  }}

  showPanel('admin-shell');

  const shareUrl = BASE + '/verify';
  document.getElementById('share-url').textContent = shareUrl;
  document.getElementById('share-open').href = shareUrl;

  await loadBatches();
}}

init();
</script>
</body>
</html>"##
    )
}
