-- 0073: boards, one row is one piece of work (Arugula #365, aru-125). A
-- row's lane follows the live state of its items and is never stored, so
-- nobody places a row in a zone: a row has no `zone_id` any more, and
-- `board_entries` holds rows (`db::boards` deletes the zone entries and
-- clears the rows' `zone_id` once, on start: `Db::lanes_move`).
--
-- Work and proof are attached to a row in `board_attachments`: `role`
-- `work` is a PR (a `forge_item`), an agent `block` or a `run`; `proof` is a
-- `run` or a `link`. The pointer's columns are a row's own (`target_kind`,
-- `target`, and `pane` for a block); `agent` is the machine whose agent
-- attached it, NULL for a person. Times are milliseconds since the epoch.
--
-- Not additive: the build before this one reads a row without a zone as no
-- row at all. SQLite changes a `CHECK` by making the table again, with every
-- column it has now (0058, `agent` from 0059).
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
    CHECK (kind = 'row' OR zone_id IS NULL),
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

CREATE TABLE board_attachments (
    id TEXT PRIMARY KEY,
    board_id TEXT NOT NULL,
    row_id TEXT NOT NULL,
    role TEXT NOT NULL CHECK (role IN ('work', 'proof')),
    target_kind TEXT NOT NULL CHECK (target_kind IN ('forge_item', 'run', 'block', 'link')),
    target TEXT NOT NULL,
    pane TEXT,
    agent TEXT,
    created_at INTEGER NOT NULL,
    CHECK ((target_kind = 'block') = (pane IS NOT NULL))
);
CREATE INDEX board_attachments_board ON board_attachments (board_id, created_at, id);
CREATE INDEX board_attachments_row ON board_attachments (row_id);
