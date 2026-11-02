-- 0087: boards link Docs (Arugula #517). A board has a strip of Docs, and
-- each row may have Docs of its own (`board_docs`): a project Doc by its
-- `pdc_` id (`target_kind` `doc`), or a Doc block on a machine
-- (`machine_doc`: `machine` and the Doc's id on it, with the `title` it had
-- when linked, since the machine may be off). `row_id` is NULL for the
-- board's strip and a row's id for that row's Docs (they go with the row).
-- Their order is `position`, from 0 within the strip or the row; `added_by`
-- is the account (the owner's, for an agent) and `agent` the machine whose
-- agent linked it, NULL for a person. `machine` is '' for a project Doc. A
-- Doc is once in a strip and once on a row: the unique index reads a NULL
-- `row_id` as '', so the strip dedupes too. Times are milliseconds since
-- the epoch.
--
-- Additive: the build before this one ignores the table.
CREATE TABLE board_docs (
    id TEXT COLLATE pg_c_utf8 PRIMARY KEY,
    board_id TEXT COLLATE pg_c_utf8 NOT NULL,
    row_id TEXT COLLATE pg_c_utf8,
    position BIGINT NOT NULL,
    target_kind TEXT COLLATE pg_c_utf8 NOT NULL CHECK (target_kind IN ('doc', 'machine_doc')),
    target TEXT COLLATE pg_c_utf8 NOT NULL,
    machine TEXT COLLATE pg_c_utf8 NOT NULL DEFAULT '',
    title TEXT COLLATE pg_c_utf8 NOT NULL DEFAULT '',
    added_by TEXT COLLATE pg_c_utf8 NOT NULL,
    agent TEXT COLLATE pg_c_utf8,
    created_at BIGINT NOT NULL
);
CREATE UNIQUE INDEX board_docs_target ON board_docs (board_id, COALESCE(row_id, ''), target_kind, target, machine);
CREATE INDEX board_docs_board ON board_docs (board_id, row_id, position);
CREATE INDEX board_docs_doc ON board_docs (target_kind, target);
