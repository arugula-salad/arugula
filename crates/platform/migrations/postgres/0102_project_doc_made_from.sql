-- 0102: a team Doc made from a machine Doc's copy (Arugula #418, Share
-- with…). See SQLite's file, in Postgres's words.
ALTER TABLE project_docs ADD COLUMN made_from_project TEXT COLLATE pg_c_utf8;
ALTER TABLE project_docs ADD COLUMN made_from_doc TEXT COLLATE pg_c_utf8;
ALTER TABLE project_docs ADD COLUMN made_from_rev BIGINT;
ALTER TABLE project_docs ADD COLUMN made_rev BIGINT;
CREATE UNIQUE INDEX project_docs_made_from ON project_docs (made_from_doc, project_id)
    WHERE made_from_doc IS NOT NULL;
