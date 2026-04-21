CREATE TABLE IF NOT EXISTS code_batches (
    id                         BIGSERIAL PRIMARY KEY,
    guild_id                   TEXT NOT NULL,
    name                       TEXT NOT NULL,
    description                TEXT,
    kind                       TEXT NOT NULL CHECK (kind IN ('unique_per_code','unique_per_user','shared_unlimited')),
    max_redemptions_per_code   INTEGER,
    max_redemptions_total      INTEGER,
    expires_at                 TIMESTAMPTZ,
    revoked_at                 TIMESTAMPTZ,
    created_by_discord_id      TEXT NOT NULL,
    created_at                 TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at                 TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (guild_id, name)
);
CREATE INDEX IF NOT EXISTS idx_code_batches_guild ON code_batches (guild_id) WHERE revoked_at IS NULL;

CREATE TABLE IF NOT EXISTS codes (
    id          BIGSERIAL PRIMARY KEY,
    batch_id    BIGINT NOT NULL REFERENCES code_batches(id) ON DELETE CASCADE,
    code        TEXT NOT NULL,
    uses_count  INTEGER NOT NULL DEFAULT 0,
    revoked_at  TIMESTAMPTZ,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (batch_id, code)
);
CREATE INDEX IF NOT EXISTS idx_codes_lookup ON codes (code);
CREATE INDEX IF NOT EXISTS idx_codes_batch ON codes (batch_id);
