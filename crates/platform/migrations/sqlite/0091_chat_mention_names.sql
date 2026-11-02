-- 0091: the session names a mention could have used (Arugula #576), when the
-- one it named wasn't there: a JSON array of strings, NULL for none.
ALTER TABLE chat_mention_status ADD COLUMN names TEXT;
