ALTER TABLE code_batches ADD COLUMN IF NOT EXISTS role_duration_hours INTEGER;

ALTER TABLE redemptions ADD COLUMN IF NOT EXISTS role_expires_at TIMESTAMPTZ;
ALTER TABLE redemptions ADD COLUMN IF NOT EXISTS role_revoked_at TIMESTAMPTZ;

CREATE INDEX IF NOT EXISTS idx_redemptions_expiry_unrevoked
    ON redemptions (role_expires_at)
    WHERE role_expires_at IS NOT NULL AND role_revoked_at IS NULL;
