-- 0054: snapshots per hosted workspace (M263, thread = session, Arugula #140).
-- SQLite's file says what each column means. Postgres changes the
-- `purpose` CHECK in place (0028 left it its default name) and adds the
-- columns.
--
-- Additive: one more `purpose` value, and columns that may be NULL or have
-- a default.
ALTER TABLE hosted_workspaces DROP CONSTRAINT hosted_workspaces_purpose_check;
ALTER TABLE hosted_workspaces ADD CONSTRAINT hosted_workspaces_purpose_check
    CHECK (purpose IN ('workspace', 'deploy', 'snapshot'));
ALTER TABLE hosted_workspaces ADD COLUMN parent_id TEXT COLLATE pg_c_utf8;
ALTER TABLE hosted_workspaces ADD COLUMN snapshot_id TEXT COLLATE pg_c_utf8;
ALTER TABLE hosted_workspaces ADD COLUMN snapshot_hash TEXT COLLATE pg_c_utf8;
ALTER TABLE hosted_workspaces ADD COLUMN snapshot_template TEXT COLLATE pg_c_utf8;
ALTER TABLE hosted_workspaces ADD COLUMN snapshot_built_at BIGINT;
ALTER TABLE hosted_workspaces ADD COLUMN snapshot_status TEXT COLLATE pg_c_utf8
    CHECK (snapshot_status IN ('building', 'ready', 'failed'));
ALTER TABLE hosted_workspaces ADD COLUMN snapshot_error TEXT COLLATE pg_c_utf8;
ALTER TABLE hosted_workspaces ADD COLUMN snapshot_try_hash TEXT COLLATE pg_c_utf8;
ALTER TABLE hosted_workspaces ADD COLUMN snapshot_try_at BIGINT;
ALTER TABLE hosted_workspaces ADD COLUMN snapshot_retired TEXT COLLATE pg_c_utf8 NOT NULL DEFAULT '[]';
ALTER TABLE hosted_workspaces ADD COLUMN box_snapshot TEXT COLLATE pg_c_utf8;
CREATE INDEX hosted_workspaces_parent ON hosted_workspaces (parent_id) WHERE parent_id IS NOT NULL;
