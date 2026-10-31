-- 0075: a PR that names an issue that is a board's row is attached to the row
-- by the platform (Arugula #368), and taken off when the PR stops naming it.
-- `source` is `auto` on those, NULL on one a person or an agent attached,
-- which the platform never takes off. Additive: the build before this one
-- reads an automatic attachment as any other.
--
-- A PR's change finds its rows by the issue they point at, and its own
-- attachments by the PR: the two indexes.
ALTER TABLE board_attachments ADD COLUMN source TEXT CHECK (source IN ('auto'));
CREATE INDEX board_entries_target ON board_entries (target_kind, target) WHERE kind = 'row';
CREATE INDEX board_attachments_target ON board_attachments (target_kind, target);
