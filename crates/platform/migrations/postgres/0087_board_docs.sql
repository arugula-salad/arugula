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
-- its `role` and `target_kind` checks widen (`reference`; `doc`,
-- `machine_doc`), and it gains `machine` and `title` for a machine Doc.
--
-- Not additive: the build before this one can't read a `reference`
-- attachment.
CREATE TABLE board_docs (
    id TEXT COLLATE pg_c_utf8 PRIMARY KEY,
    board_id TEXT COLLATE pg_c_utf8 NOT NULL,
    position BIGINT NOT NULL,
    target_kind TEXT COLLATE pg_c_utf8 NOT NULL CHECK (target_kind IN ('doc', 'machine_doc')),
    target TEXT COLLATE pg_c_utf8 NOT NULL,
    machine TEXT COLLATE pg_c_utf8 NOT NULL DEFAULT '',
    title TEXT COLLATE pg_c_utf8 NOT NULL DEFAULT '',
    added_by TEXT COLLATE pg_c_utf8 NOT NULL,
    agent TEXT COLLATE pg_c_utf8,
    created_at BIGINT NOT NULL
);
CREATE UNIQUE INDEX board_docs_target ON board_docs (board_id, target_kind, target, machine);
CREATE INDEX board_docs_board ON board_docs (board_id, position);
CREATE INDEX board_docs_doc ON board_docs (target_kind, target);

ALTER TABLE board_attachments DROP CONSTRAINT board_attachments_role_check;
ALTER TABLE board_attachments ADD CONSTRAINT board_attachments_role_check
    CHECK (role IN ('work', 'proof', 'reference'));
ALTER TABLE board_attachments DROP CONSTRAINT board_attachments_target_kind_check;
ALTER TABLE board_attachments ADD CONSTRAINT board_attachments_target_kind_check
    CHECK (target_kind IN ('forge_item', 'run', 'block', 'link', 'doc', 'machine_doc'));
ALTER TABLE board_attachments ADD COLUMN machine TEXT COLLATE pg_c_utf8;
ALTER TABLE board_attachments ADD COLUMN title TEXT COLLATE pg_c_utf8;
ALTER TABLE board_attachments ADD CONSTRAINT board_attachments_machine_doc_check
    CHECK ((target_kind = 'machine_doc') = (machine IS NOT NULL));
