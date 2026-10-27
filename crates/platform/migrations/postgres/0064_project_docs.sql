-- 0064: project Docs (aru-118). A project's own Doc (`pdc_` and 32 hex
-- characters): a title and an ordered list of sections, each a heading and
-- its Markdown, kept by the platform and read by the project's members, as
-- its issues are. Plaintext, like issues and chat. Times are milliseconds
-- since the epoch. `db::project_docs` has the rules.
-- SQLite's file in Postgres's words: `BIGINT` for its integers and
-- `COLLATE pg_c_utf8` text.

-- A Doc. `made_by` is the account of the person who made it, empty once
-- that account is deleted; a Doc an agent session made has none, and its
-- session in `made_by_session` (aru-115). `rev` counts its changes.
CREATE TABLE project_docs (
    id TEXT COLLATE pg_c_utf8 PRIMARY KEY,
    project_id TEXT COLLATE pg_c_utf8 NOT NULL,
    title TEXT COLLATE pg_c_utf8 NOT NULL,
    made_by TEXT COLLATE pg_c_utf8 NOT NULL,
    made_by_session TEXT COLLATE pg_c_utf8,
    rev BIGINT NOT NULL DEFAULT 1,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL
);
CREATE INDEX project_docs_project ON project_docs (project_id, updated_at);

-- A Doc's sections. `id` is the Doc's own (`s1`, `s2`, …, never reused);
-- `position` orders them from 0. `intent` is one line on what a pending
-- section will hold; `body` is its Markdown, NULL while it's pending.
-- `rev` counts the writes to its body.
CREATE TABLE project_doc_sections (
    doc_id TEXT COLLATE pg_c_utf8 NOT NULL,
    id TEXT COLLATE pg_c_utf8 NOT NULL,
    position BIGINT NOT NULL,
    heading TEXT COLLATE pg_c_utf8 NOT NULL,
    intent TEXT COLLATE pg_c_utf8 NOT NULL DEFAULT '',
    body TEXT COLLATE pg_c_utf8,
    rev BIGINT NOT NULL DEFAULT 0,
    updated_at BIGINT NOT NULL,
    PRIMARY KEY (doc_id, id)
);
