ALTER TABLE redemptions ADD COLUMN IF NOT EXISTS pending BOOLEAN NOT NULL DEFAULT FALSE;
CREATE INDEX IF NOT EXISTS idx_redemptions_pending
    ON redemptions (redeemed_at)
    WHERE pending = TRUE;

ALTER TABLE code_batches ADD COLUMN IF NOT EXISTS invite_url TEXT;
