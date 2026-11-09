-- 0100: when a linked repo's last full pass started. About once a day the
-- forge mirror's poller follows a reconcile with a full pass, which finds
-- items GitHub deleted or moved (a reconcile only sees what was updated)
-- and reads the stalest rows again. NULL: none yet (the next is due from
-- the repo's backfill).
--
-- A repo linked before this starts as if its last pass came with its last
-- reconcile, so each repo's first pass comes at its own point of the next
-- day, not every repo's at once after the deploy.
ALTER TABLE linked_repos ADD COLUMN full_at INTEGER;
UPDATE linked_repos SET full_at = COALESCE(reconciled_at, backfilled_at);
