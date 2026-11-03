-- 0092: project Docs keep pictures, and where a copy came from (Arugula
-- #418). A machine copies its Docs up to its owner's personal project; the
-- machine and its own Doc id are kept, so the same machine Doc always lands
-- on the same `pdc_`. Both are NULL for a Doc made on the platform.
-- SQLite's file in Postgres's words.
ALTER TABLE project_docs ADD COLUMN machine TEXT COLLATE pg_c_utf8;
ALTER TABLE project_docs ADD COLUMN machine_doc TEXT COLLATE pg_c_utf8;
CREATE UNIQUE INDEX project_docs_machine ON project_docs (machine, machine_doc)
    WHERE machine IS NOT NULL AND machine_doc IS NOT NULL;

-- A Doc's pictures, the bytes themselves, as chat's files are. `name` is a
-- plain file name the Doc's sections cite as `assets/<name>`; `mime` is what
-- the bytes sniff as (PNG, JPEG, GIF or WebP); `rev` changes whenever the
-- name is written with other bytes. A picture goes with its Doc.
CREATE TABLE project_doc_assets (
    doc_id TEXT COLLATE pg_c_utf8 NOT NULL,
    name TEXT COLLATE pg_c_utf8 NOT NULL,
    mime TEXT COLLATE pg_c_utf8 NOT NULL,
    size BIGINT NOT NULL,
    rev BIGINT NOT NULL,
    bytes BYTEA NOT NULL,
    updated_at BIGINT NOT NULL,
    PRIMARY KEY (doc_id, name)
);
