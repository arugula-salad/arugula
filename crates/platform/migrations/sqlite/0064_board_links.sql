-- 0064: a board row may point at a link (boards, evidence on PROOFS rows,
-- Arugula #191). `target_kind` gains `link`, and `target` holds the URL
-- (`https` only, at most 2048 bytes, no userinfo: `boards::pointer_ok`
-- has the rules). Nothing else changes.
--
-- Not additive: an older build reads a `link` row as a pointer it doesn't
-- know and fails to read the board, so it can't be rolled back to once one
-- is written. SQLite changes a `CHECK` by making the table again, with
-- every column it has now (0058, and `agent` from 0059).
CREATE TABLE board_entries_new (
    id TEXT PRIMARY KEY,
    board_id TEXT NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('zone', 'row')),
    zone_id TEXT,
    position INTEGER NOT NULL,
    rev INTEGER NOT NULL DEFAULT 1,
    title TEXT NOT NULL DEFAULT '',
    zone_kind TEXT CHECK (zone_kind IN ('proofs')),
    note TEXT NOT NULL DEFAULT '',
    status TEXT CHECK (status IN ('open', 'working', 'needs_input', 'done', 'failed')),
    target_kind TEXT CHECK (target_kind IN ('forge_item', 'issue', 'run', 'workspace', 'block', 'link')),
    target TEXT,
    pane TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    agent TEXT,
    CHECK ((kind = 'row') = (zone_id IS NOT NULL)),
    CHECK (kind = 'zone' OR zone_kind IS NULL),
    CHECK (kind = 'row' OR (note = '' AND status IS NULL AND target_kind IS NULL)),
    CHECK ((target_kind IS NULL) = (target IS NULL)),
    CHECK ((target_kind = 'block') = (pane IS NOT NULL))
);
INSERT INTO board_entries_new (id, board_id, kind, zone_id, position, rev, title, zone_kind, note, status,
        target_kind, target, pane, created_at, updated_at, agent)
    SELECT id, board_id, kind, zone_id, position, rev, title, zone_kind, note, status,
        target_kind, target, pane, created_at, updated_at, agent
    FROM board_entries;
DROP TABLE board_entries;
ALTER TABLE board_entries_new RENAME TO board_entries;
CREATE INDEX board_entries_board ON board_entries (board_id, kind, zone_id, position);
