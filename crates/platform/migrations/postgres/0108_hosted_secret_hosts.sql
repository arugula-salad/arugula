-- 0108: the hosts a workspace's clone secret may be sent to. See SQLite's
-- file, in Postgres's words.
ALTER TABLE hosted_workspace_secrets ADD COLUMN hosts TEXT COLLATE pg_c_utf8;
