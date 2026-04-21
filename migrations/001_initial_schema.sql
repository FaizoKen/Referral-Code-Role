-- Role links: one per guild+role pair registered via POST /register
CREATE TABLE IF NOT EXISTS role_links (
    id          BIGSERIAL PRIMARY KEY,
    guild_id    TEXT NOT NULL,
    role_id     TEXT NOT NULL,
    api_token   TEXT NOT NULL UNIQUE,
    conditions  JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (guild_id, role_id)
);

-- Local mirror of who currently holds the role (for diffing on revoke).
CREATE TABLE IF NOT EXISTS role_assignments (
    guild_id    TEXT NOT NULL,
    role_id     TEXT NOT NULL,
    discord_id  TEXT NOT NULL,
    assigned_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (guild_id, role_id, discord_id),
    FOREIGN KEY (guild_id, role_id) REFERENCES role_links (guild_id, role_id) ON DELETE CASCADE
);

-- OAuth state CSRF table (cleaned by background task).
CREATE TABLE IF NOT EXISTS oauth_states (
    state         TEXT PRIMARY KEY,
    redirect_data JSONB,
    expires_at    TIMESTAMPTZ NOT NULL,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);
