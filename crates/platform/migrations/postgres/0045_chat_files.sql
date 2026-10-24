-- 0045: files and images in chat (#107). Additive: one new table, nothing
-- existing is touched. Times are milliseconds since the epoch.
--
-- SQLite's file in Postgres's words: `BIGINT` for its integers, `BYTEA`
-- for its blob and `COLLATE pg_c_utf8` text.

-- See SQLite's file.
CREATE TABLE chat_files (
    id TEXT COLLATE pg_c_utf8 PRIMARY KEY,
    room TEXT COLLATE pg_c_utf8 NOT NULL,
    seq BIGINT,
    ord BIGINT NOT NULL DEFAULT 0,
    account TEXT COLLATE pg_c_utf8 NOT NULL,
    scope TEXT COLLATE pg_c_utf8 NOT NULL,
    name TEXT COLLATE pg_c_utf8 NOT NULL,
    mime TEXT COLLATE pg_c_utf8 NOT NULL,
    kind TEXT COLLATE pg_c_utf8 NOT NULL,
    size BIGINT NOT NULL,
    width BIGINT,
    height BIGINT,
    at BIGINT NOT NULL,
    data BYTEA NOT NULL
);
CREATE INDEX chat_files_message ON chat_files (room, seq);
CREATE INDEX chat_files_scope ON chat_files (scope);
CREATE INDEX chat_files_account ON chat_files (account);
