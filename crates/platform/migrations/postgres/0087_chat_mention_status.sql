-- 0087: where an @machine mention in chat got to (Arugula #502). Times are
-- milliseconds since the epoch. `db::chat_mention_status` has the rules.
--
-- SQLite's file in Postgres's words: `BIGINT` for its integers and
-- `COLLATE pg_c_utf8` text.

CREATE TABLE chat_mention_status (
    room TEXT COLLATE pg_c_utf8 NOT NULL,
    seq BIGINT NOT NULL,
    machine TEXT COLLATE pg_c_utf8 NOT NULL,
    machine_name TEXT COLLATE pg_c_utf8 NOT NULL DEFAULT '',
    root BIGINT NOT NULL,
    state TEXT COLLATE pg_c_utf8 NOT NULL,
    session TEXT COLLATE pg_c_utf8,
    asked TEXT COLLATE pg_c_utf8,
    reply BIGINT,
    at BIGINT NOT NULL,
    PRIMARY KEY (room, seq, machine)
);
CREATE INDEX chat_mention_status_root ON chat_mention_status (room, root, state);
