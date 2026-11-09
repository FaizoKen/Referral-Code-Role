-- The member's Discord display name at the moment they redeemed (from their
-- session), so the admin page's "Who redeemed" list and its CSV can name
-- people instead of showing bare IDs. Rows from before this migration have
-- none; the page shows their ID alone.
ALTER TABLE redemptions ADD COLUMN IF NOT EXISTS display_name TEXT;

-- "Per-person codes" allow one redemption per member across the whole group,
-- so a redeem asks "has this member redeemed anything in this group?". This
-- index answers it without scanning the group's redemptions.
CREATE INDEX IF NOT EXISTS idx_redemptions_batch_user ON redemptions (batch_id, discord_id);
