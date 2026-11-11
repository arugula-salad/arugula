-- 0107: a project Doc's layout (Arugula #826). See SQLite's file, in
-- Postgres's words.
ALTER TABLE project_docs ADD COLUMN layout TEXT COLLATE pg_c_utf8;
