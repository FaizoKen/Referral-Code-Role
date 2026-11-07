-- A code is identified by its letters and digits alone. Hyphens are display:
-- a code generated with the prefix "LAT-" is stored as "LAT-7XK2M9QH4PRT"
-- and must redeem whether it is typed with or without the hyphen. Matching
-- the stored text against hyphen-stripped input never found those codes.
--
-- The key is a generated column so every writer stores it without knowing it
-- exists, including an older image after a rollback, and the codes created
-- before this migration get theirs when the column is added. It must stay
-- equal to code_gen::code_key: ASCII letters upper-cased, everything that is
-- not A-Z or 0-9 dropped. COLLATE "C" keeps upper() to ASCII in any locale.
ALTER TABLE codes ADD COLUMN IF NOT EXISTS code_key TEXT
    GENERATED ALWAYS AS (regexp_replace(upper(code COLLATE "C"), '[^A-Z0-9]', '', 'g')) STORED;

-- (code_key, batch_id) serves the redeem lookup (code_key leads) and stops a
-- group holding two codes that differ only by hyphens, which would let one
-- "one-time" code be claimed twice. If a group already holds such a pair, the
-- unique index cannot be built; fall back to a plain lookup index rather than
-- failing startup, and try again on the next start.
DO $$
BEGIN
    IF to_regclass('uq_codes_key_batch') IS NULL THEN
        IF EXISTS (
            SELECT 1 FROM codes GROUP BY code_key, batch_id HAVING COUNT(*) > 1
        ) THEN
            RAISE WARNING 'codes: a group holds codes that differ only by separators; code_key is indexed without uniqueness';
            CREATE INDEX IF NOT EXISTS idx_codes_key ON codes (code_key);
        ELSE
            CREATE UNIQUE INDEX uq_codes_key_batch ON codes (code_key, batch_id);
            DROP INDEX IF EXISTS idx_codes_key;
        END IF;
    END IF;
END $$;

-- Nothing looks a code up by its stored text any more (the old lookup used
-- UPPER(code), which this index could not serve, so every redeem attempt was
-- a full scan). 002 no longer creates it.
DROP INDEX IF EXISTS idx_codes_lookup;
