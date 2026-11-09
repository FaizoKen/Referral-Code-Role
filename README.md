# Referral Code Role

A [RoleLogic](https://docs-rolelogic.faizo.net) plugin that grants a Discord role when a member redeems a code. Hand out codes for Kickstarter rewards, podcast shout-outs, event wristbands, QR-coded flyers — anything that needs a "prove you have this and get the role" flow.

Built for scale: the Role Link sync path supports **100 000+ members per role link** via the chunked upload API, with atomic swaps.

---

## How it works

```
┌─────────────┐        ┌───────────────────────┐        ┌─────────────────┐
│  Admin (UI) │──────▶ │  /admin  (this server)│        │   RoleLogic API │
└─────────────┘        │                       │        │   (upstream)    │
                       │  ┌─────────────────┐  │        └─────────────────┘
┌─────────────┐        │  │  code_batches   │  │                ▲
│  Member UI  │──────▶ │  │  codes          │  │   sync users   │
└─────────────┘        │  │  redemptions    │──┼───────────────▶│
       │               │  └─────────────────┘  │                │
       │               │                       │                │
       ▼               │  Sign-in via session  │                │
┌─────────────┐        │  cookie issued by the │                │
│ Auth Gateway│◀───────│  centralized Auth GW  │                │
└─────────────┘        └───────────────────────┘                │
                              GET /register, /config ◀──────────┘
```

1. An admin creates a **code group** (a batch with rules: its type, an optional total redemption cap, expiry, role duration and Discord invite link). Groups can be edited later; their type can't.
2. The admin generates codes (random or custom). Each code has a share URL and a downloadable QR; a group's codes and its redemptions also download as CSV.
3. A member opens the share URL, signs in through the Auth Gateway (Discord OAuth), enters a code.
4. The plugin validates the code, records the redemption, and pushes the updated member list to the RoleLogic API so the bot grants the role. A group gives a role only once it is ticked in that role's settings in the RoleLogic dashboard; the admin page flags groups no role has ticked.

Codes can be **time-limited** — if a batch has `role_duration_hours` set, the role is automatically revoked when it expires (a background worker sweeps every 60s).

---

## Batch kinds

| Kind | Behavior |
|------|----------|
| `unique_per_code` | "One-time codes": each code can be used once, by one user. |
| `unique_per_user` | "Per-person codes": a code can be used by many people (print one on every wristband), but each person redeems **once per group**, whichever of its codes they hold. (`max_redemptions_per_code` still caps a code when set through the API.) |
| `shared_unlimited` | "Single shared code": one code, many users, once each — until the batch `max_redemptions_total` or `expires_at` is hit. |

The limits are checked again inside the redeem's transaction with the code's row (and, for a cap or per-person group, the group's row) locked, so two people can't both take the last use.

Retiring a group stops its codes; people who already redeemed keep the role. A retired group a role still has ticked stays in that role's picker, labelled "(retired)", so its settings still save (the dashboard only saves values it offers).

---

## Endpoints

All routes are nested under the plugin slug `/referral-code-role`.

### RoleLogic contract (upstream calls the plugin)
| Method | Path | Purpose |
|--------|------|---------|
| `POST` | `/register` | Store the role-link API token |
| `GET`  | `/config` | Return the dashboard schema + current values |
| `POST` | `/config` | Persist the config submitted from the dashboard |
| `DELETE` | `/config` | Tear down a role link |
| `GET`  | `/health` | Liveness + DB probe |

### Admin API (session-authenticated, manager-only)
| Method | Path | Purpose |
|--------|------|---------|
| `GET`  | `/admin` | Admin single-page UI |
| `GET`  | `/admin/api/stats` | Guild summary |
| `GET` `POST` | `/admin/api/batches` | List / create code groups |
| `PATCH` `DELETE` | `/admin/api/batches/:id` | Update (`null` clears a field) / retire a group |
| `GET` `POST` | `/admin/api/batches/:id/codes` | List / generate codes |
| `GET`  | `/admin/api/batches/:id/redemptions` | Redemption history (names, role end times) |
| `GET`  | `/admin/api/batches/:id/codes.csv` | Every code with its redemption link |
| `GET`  | `/admin/api/batches/:id/redemptions.csv` | Everyone who redeemed |
| `DELETE` | `/admin/api/codes/:id` | Revoke a single code |
| `GET`  | `/admin/api/codes/:id/qr.svg` | Code QR as SVG |

### Member redemption (session-authenticated)
| Method | Path | Purpose |
|--------|------|---------|
| `GET`  | `/verify` | Redemption page |
| `GET`  | `/verify/login` | Kick off Discord OAuth via the gateway |
| `GET`  | `/verify/status` | Who-am-I for the logged-in session |
| `POST` | `/verify/redeem` | Redeem a code |
| `POST` | `/verify/refresh` | Refresh guild membership + pending flag |
| `GET`  | `/verify/me/redemptions` | User's own redemption history |
| `POST` | `/verify/logout` | Clear the session cookie |

---

## Running locally

```bash
cp .env.example .env
# Fill in SESSION_SECRET, BASE_URL, INTERNAL_API_KEY, AUTH_GATEWAY_URL.
# SESSION_SECRET must match the Auth Gateway's value.

docker compose up --build
```

The server listens on `:8080` inside the container. Put it behind the same reverse proxy / Cloudflare Tunnel as the rest of the RoleLogic plugin fleet; the public URL must:

- use HTTPS
- end in `/referral-code-role` (the RoleLogic register flow posts to `<BASE_URL>/register` etc.)

### Non-Docker

```bash
cargo run --release
```

Requires a reachable Postgres 14+; migrations apply automatically on boot from [migrations/](migrations/).

---

## Configuration

See [.env.example](.env.example) for the annotated list. The essentials:

| Var | Purpose |
|-----|---------|
| `DATABASE_URL` | Postgres connection string |
| `SESSION_SECRET` | HMAC key for session cookies — must match Auth Gateway |
| `BASE_URL` | Public URL of this plugin, ending in `/referral-code-role` |
| `AUTH_GATEWAY_URL` | Internal URL of the Auth Gateway |
| `INTERNAL_API_KEY` | Shared secret for `/auth/internal/*` calls |
| `MAX_REDEEM_ATTEMPTS_PER_HOUR` | Per-user redeem rate limit (default 20) |
| `DATABASE_MIN_CONNECTIONS` / `DATABASE_MAX_CONNECTIONS` | Pool sizing (defaults 4 / 32) |

---

## Scaling

Base defaults target small-to-medium guilds. For role links with 100k+ qualifying members:

- **Role Link sync** automatically switches from `PUT /users` to the atomic chunked-upload flow (`POST /users/upload` → `chunk` → `commit`) — see [src/services/rolelogic.rs](src/services/rolelogic.rs). No config needed.
- **Postgres pool** defaults to max 32 connections. Raise `DATABASE_MAX_CONNECTIONS` and bump `max_connections` in your Postgres config in lockstep.
- **Postgres tuning** — the bundled [compose.yml](compose.yml) is sized for small deployments. For large guilds raise `shared_buffers`, `work_mem`, `effective_cache_size`, and the container memory limits. See `.claude/BLUEPRINT.md` Section 17 for the full large-tier recipe.
- **Event buffers** — `player_sync_tx` is sized at 4096 and `config_sync_tx` at 256; redeem bursts during launches won't drop events at those sizes.
- **Role expiry** and **pending-redemption** workers page in batches of 500 every 60–120s; raise `BATCH_SIZE` in [src/tasks/](src/tasks/) if you need faster convergence.

For the full Role Link API contract see <https://docs-rolelogic.faizo.net/reference/role-link-api>.

---

## Data model

Migrations live in [migrations/](migrations/). High level:

- `role_links` — one row per (guild, role) registered via `/register`; stores the API token and the conditions JSON.
- `code_batches` — admin-created code groups with kind / caps / expiry / optional `role_duration_hours` / optional `invite_url`.
- `codes` — individual redeemable codes inside a batch.
- `redemptions` — who redeemed what, when (with their Discord display name at the time); also tracks `pending` (redeemed before joining the guild) and `role_expires_at` / `role_revoked_at` for time-limited roles.
- `redemption_attempts` — rate-limit log (success and failure).
- `role_assignments` — local mirror of who holds each role, used for diffing.
- `oauth_states` — short-lived CSRF tokens for the sign-in flow.

---

## License

See [LICENSE](LICENSE).
