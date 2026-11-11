-- 0109: the machine whose agent made a project Doc (Arugula #862). See
-- SQLite's file, in Postgres's words.
ALTER TABLE project_docs ADD COLUMN made_by_agent TEXT COLLATE pg_c_utf8;
