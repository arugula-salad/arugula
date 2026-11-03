-- 0091: the session names a mention could have used (Arugula #576), when the
-- one it named wasn't there: a JSON array of strings, NULL for none.
-- SQLite's file in Postgres's words.
ALTER TABLE chat_mention_status ADD COLUMN names TEXT COLLATE pg_c_utf8;
