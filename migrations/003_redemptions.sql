CREATE TABLE IF NOT EXISTS redemptions (
    id          BIGSERIAL PRIMARY KEY,
    code_id     BIGINT NOT NULL REFERENCES codes(id) ON DELETE CASCADE,
    batch_id    BIGINT NOT NULL REFERENCES code_batches(id) ON DELETE CASCADE,
    guild_id    TEXT NOT NULL,
    discord_id  TEXT NOT NULL,
    redeemed_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    ip_hash     TEXT,
    user_agent  TEXT,
    UNIQUE (code_id, discord_id)
);
CREATE INDEX IF NOT EXISTS idx_redemptions_user ON redemptions (discord_id);
CREATE INDEX IF NOT EXISTS idx_redemptions_batch ON redemptions (batch_id);
CREATE INDEX IF NOT EXISTS idx_redemptions_guild_user ON redemptions (guild_id, discord_id);

CREATE TABLE IF NOT EXISTS redemption_attempts (
    id             BIGSERIAL PRIMARY KEY,
    discord_id     TEXT,
    ip_hash        TEXT,
    attempted_code TEXT NOT NULL,
    success        BOOLEAN NOT NULL,
    attempted_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_attempts_recent ON redemption_attempts (discord_id, attempted_at DESC);
CREATE INDEX IF NOT EXISTS idx_attempts_ip ON redemption_attempts (ip_hash, attempted_at DESC);
