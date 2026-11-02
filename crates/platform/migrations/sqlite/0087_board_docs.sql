-- 0087: boards link Docs (Arugula #517). A board has a strip of Docs
-- (`board_docs`): a project Doc by its `pdc_` id (`target_kind` `doc`), or
-- a Doc block on a machine (`machine_doc`: `machine` and the Doc's id on it,
-- with the `title` it had when linked, since the machine may be off). Their
-- order is `position`, from 0; `added_by` is the account (the owner's, for
-- an agent) and `agent` the machine whose agent linked it, NULL for a
-- person. `machine` is '' for a project Doc, so one Doc is in a strip once.
-- Times are milliseconds since the epoch.
--
-- A Doc is also attached to one row, as a `reference` in `board_attachments`:
-- the table widens `role` (`reference`) and `target_kind` (`doc`,
-- `machine_doc`), and gains `machine` and `title` for a machine Doc. SQLite
-- changes a `CHECK` by making the table again, with every column it has now
-- (`source` from 0075, `released_at` from 0079).
--
-- Not additive: the build before this one can't read a `reference`
-- attachment.
CREATE TABLE board_docs (
    id TEXT PRIMARY KEY,
    board_id TEXT NOT NULL,
    position INTEGER NOT NULL,
    target_kind TEXT NOT NULL CHECK (target_kind IN ('doc', 'machine_doc')),
    target TEXT NOT NULL,
    machine TEXT NOT NULL DEFAULT '',
    title TEXT NOT NULL DEFAULT '',
    added_by TEXT NOT NULL,
    agent TEXT,
    created_at INTEGER NOT NULL
);
CREATE UNIQUE INDEX board_docs_target ON board_docs (board_id, target_kind, target, machine);
CREATE INDEX board_docs_board ON board_docs (board_id, position);
CREATE INDEX board_docs_doc ON board_docs (target_kind, target);

CREATE TABLE board_attachments_new (
    id TEXT PRIMARY KEY,
    board_id TEXT NOT NULL,
    row_id TEXT NOT NULL,
    role TEXT NOT NULL CHECK (role IN ('work', 'proof', 'reference')),
    target_kind TEXT NOT NULL CHECK (target_kind IN ('forge_item', 'run', 'block', 'link', 'doc', 'machine_doc')),
    target TEXT NOT NULL,
    pane TEXT,
    agent TEXT,
    created_at INTEGER NOT NULL,
    source TEXT CHECK (source IN ('auto')),
    released_at INTEGER,
    machine TEXT,
    title TEXT,
    CHECK ((target_kind = 'block') = (pane IS NOT NULL)),
    CHECK ((target_kind = 'machine_doc') = (machine IS NOT NULL))
);
INSERT INTO board_attachments_new (id, board_id, row_id, role, target_kind, target, pane, agent, created_at,
        source, released_at)
    SELECT id, board_id, row_id, role, target_kind, target, pane, agent, created_at, source, released_at
    FROM board_attachments;
DROP TABLE board_attachments;
ALTER TABLE board_attachments_new RENAME TO board_attachments;
CREATE INDEX board_attachments_board ON board_attachments (board_id, created_at, id);
CREATE INDEX board_attachments_row ON board_attachments (row_id);
CREATE INDEX board_attachments_target ON board_attachments (target_kind, target);
