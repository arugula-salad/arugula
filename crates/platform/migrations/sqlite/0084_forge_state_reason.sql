-- 0084: a GitHub issue's `state_reason` (Arugula #433): `completed`,
-- `not_planned` or `reopened`. A closed issue with `not_planned` is canceled
-- on a board. Additive: the build before this one ignores the column, and
-- rows mirrored before it stay NULL (read as completed) until a re-sync.
ALTER TABLE forge_items ADD COLUMN state_reason TEXT;
