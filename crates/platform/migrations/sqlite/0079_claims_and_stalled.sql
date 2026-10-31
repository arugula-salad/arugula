-- 0079: claiming work, and a stalled row (Arugula #404). A claim is the work
-- attachment of a block that isn't closed, so it has no table; what it adds
-- is on the attachment and the report:
--   - `board_attachments.released_at`: set when a newer agent took the row
--     over (`take`), so the old block reads as an earlier agent while it
--     still runs. NULL: not released.
--   - `board_block_states.state_since`: when the block's reported state last
--     changed, kept by the report (a repeated report of the same state keeps
--     it), so "idle for 42m" is worked out at read time. NULL reads as
--     `updated_at`.
--   - `board_settings`: a project's `stall_minutes`, how long a Doing row's
--     blocks may sit idle before it is stalled; no row means the default.
-- Additive: the build before this one ignores the columns and the table.
ALTER TABLE board_attachments ADD COLUMN released_at INTEGER;
ALTER TABLE board_block_states ADD COLUMN state_since INTEGER;
CREATE TABLE board_settings (
    project_id TEXT PRIMARY KEY,
    stall_minutes INTEGER NOT NULL CHECK (stall_minutes BETWEEN 1 AND 10080),
    updated_at INTEGER NOT NULL
);
