-- 0100: when a linked repo's last full pass started. See SQLite's file.
ALTER TABLE linked_repos ADD COLUMN full_at BIGINT;
UPDATE linked_repos SET full_at = COALESCE(reconciled_at, backfilled_at);
