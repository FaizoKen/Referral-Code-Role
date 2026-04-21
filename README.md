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

1. An admin creates a **code group** (a batch with rules: per-code caps, per-user caps, expiry, optional role duration, optional Discord invite link).
2. The admin generates codes (random or custom). Each code has a share URL and a downloadable QR.
3. A member opens the share URL, signs in through the Auth Gateway (Discord OAuth), enters a code.
4. The plugin validates the code, records the redemption, and pushes the updated member list to the RoleLogic API so the bot grants the role.

Codes can be **time-limited** — if a batch has `role_duration_hours` set, the role is automatically revoked when it expires (a background worker sweeps every 60s).

---

## Batch kinds

| Kind | Behavior |
|------|----------|
| `unique_per_code` | Each code can be used once, by one user. |
| `unique_per_user` | Each code can be used up to `max_redemptions_per_code` times, but only once per user. |
| `shared_unlimited` | One code, many users — until the batch `max_redemptions_total` or `expires_at` is hit. |

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
| `PATCH` `DELETE` | `/admin/api/batches/:id` | Update / revoke a group |
| `GET` `POST` | `/admin/api/batches/:id/codes` | List / generate codes |
| `GET`  | `/admin/api/batches/:id/redemptions` | Redemption history |
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
- `redemptions` — who redeemed what, when; also tracks `pending` (redeemed before joining the guild) and `role_expires_at` / `role_revoked_at` for time-limited roles.
- `redemption_attempts` — rate-limit log (success and failure).
- `role_assignments` — local mirror of who holds each role, used for diffing.
- `oauth_states` — short-lived CSRF tokens for the sign-in flow.

---

## License

See [LICENSE](LICENSE).
