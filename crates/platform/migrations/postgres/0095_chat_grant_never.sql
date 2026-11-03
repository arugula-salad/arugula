-- 0095: an owner said "never for this room" to a mention of their machine
-- that its agents can't read (Arugula #578). See SQLite's file.

CREATE TABLE chat_grant_never (
    room TEXT COLLATE pg_c_utf8 NOT NULL,
    machine TEXT COLLATE pg_c_utf8 NOT NULL,
    at BIGINT NOT NULL,
    PRIMARY KEY (room, machine)
);
