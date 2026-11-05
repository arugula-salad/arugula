-- 0098: a hosted machine the hub says it removed (Arugula #744). See
-- SQLite's file. Its words in Postgres's: `BIGINT` for its integers and
-- `COLLATE pg_c_utf8` text.
CREATE TABLE hosted_removed_machines (
    workspace TEXT COLLATE pg_c_utf8 PRIMARY KEY,
    machine TEXT COLLATE pg_c_utf8 NOT NULL,
    at BIGINT NOT NULL
);
