-- 0064: project Docs (aru-118). A project's own Doc (`pdc_` and 32 hex
-- characters): a title and an ordered list of sections, each a heading and
-- its Markdown, kept by the platform and read by the project's members, as
-- its issues are. Plaintext, like issues and chat. Times are milliseconds
-- since the epoch. `db::project_docs` has the rules.

-- A Doc. `made_by` is the account of the person who made it, empty once
-- that account is deleted; a Doc an agent session made has none, and its
-- session in `made_by_session` (aru-115). `rev` counts its changes.
CREATE TABLE project_docs (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL,
    title TEXT NOT NULL,
    made_by TEXT NOT NULL,
    made_by_session TEXT,
    rev INTEGER NOT NULL DEFAULT 1,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE INDEX project_docs_project ON project_docs (project_id, updated_at);

-- A Doc's sections. `id` is the Doc's own (`s1`, `s2`, …, never reused);
-- `position` orders them from 0. `intent` is one line on what a pending
-- section will hold; `body` is its Markdown, NULL while it's pending.
-- `rev` counts the writes to its body.
CREATE TABLE project_doc_sections (
    doc_id TEXT NOT NULL,
    id TEXT NOT NULL,
    position INTEGER NOT NULL,
    heading TEXT NOT NULL,
    intent TEXT NOT NULL DEFAULT '',
    body TEXT,
    rev INTEGER NOT NULL DEFAULT 0,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (doc_id, id)
);
