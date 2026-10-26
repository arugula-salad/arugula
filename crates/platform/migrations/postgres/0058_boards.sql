-- 0058: boards (live status board, step 1). A board is a project's object
-- (`brd_` and 32 hex characters): an ordered list of zones, each an
-- ordered list of rows. A board gets a thread like an issue does (`brd_`
-- in `chat::objects`, an object thread of kind `object`). Times are
-- milliseconds since the epoch.
--
-- Behind the `boards` Labs flag. `db::boards` has the rules.
-- SQLite's file in Postgres's words: `BIGINT` for its integers and
-- `COLLATE pg_c_utf8` text.

-- A board. `created_by` is the person's account; empty once that account
-- is deleted.
CREATE TABLE boards (
    id TEXT COLLATE pg_c_utf8 PRIMARY KEY,
    project_id TEXT COLLATE pg_c_utf8 NOT NULL,
    title TEXT COLLATE pg_c_utf8 NOT NULL,
    created_by TEXT COLLATE pg_c_utf8 NOT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL
);
CREATE INDEX boards_project ON boards (project_id, created_at);

-- A board's zones and rows. `id` is `zon_` or `row_` and 32 hex
-- characters. `kind` says which. A row names its zone in `zone_id`; a zone
-- has none. `position` orders siblings (a board's zones, a zone's rows)
-- from 0, renumbered whole by a move. `rev` starts at 1 and counts the
-- changes to what the entry says (title, pointer, note); a write names the
-- one it read and is refused if it is stale. A status change and a move
-- leave it alone, so they never conflict with a note.
--
-- A zone has a `title`, and `zone_kind` `proofs` for the PROOFS zone,
-- always the board's last. A row has a `note` (one line), a `status`
-- (NULL: none) and a pointer: `target_kind` with `target` (an id: a forge
-- item `fi_`, an issue `iss_`, a run `run_`, a Cloud workspace `hw_`; for
-- a `block` the machine) and, for a block only, its `pane`. No pointer:
-- free text. Nothing here resolves a pointer.
CREATE TABLE board_entries (
    id TEXT COLLATE pg_c_utf8 PRIMARY KEY,
    board_id TEXT COLLATE pg_c_utf8 NOT NULL,
    kind TEXT COLLATE pg_c_utf8 NOT NULL CHECK (kind IN ('zone', 'row')),
    zone_id TEXT COLLATE pg_c_utf8,
    position BIGINT NOT NULL,
    rev BIGINT NOT NULL DEFAULT 1,
    title TEXT COLLATE pg_c_utf8 NOT NULL DEFAULT '',
    zone_kind TEXT COLLATE pg_c_utf8 CHECK (zone_kind IN ('proofs')),
    note TEXT COLLATE pg_c_utf8 NOT NULL DEFAULT '',
    status TEXT COLLATE pg_c_utf8 CHECK (status IN ('open', 'working', 'needs_input', 'done', 'failed')),
    target_kind TEXT COLLATE pg_c_utf8 CHECK (target_kind IN ('forge_item', 'issue', 'run', 'workspace', 'block')),
    target TEXT COLLATE pg_c_utf8,
    pane TEXT COLLATE pg_c_utf8,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    CHECK ((kind = 'row') = (zone_id IS NOT NULL)),
    CHECK (kind = 'zone' OR zone_kind IS NULL),
    CHECK (kind = 'row' OR (note = '' AND status IS NULL AND target_kind IS NULL)),
    CHECK ((target_kind IS NULL) = (target IS NULL)),
    CHECK ((target_kind = 'block') = (pane IS NOT NULL))
);
CREATE INDEX board_entries_board ON board_entries (board_id, kind, zone_id, position);
