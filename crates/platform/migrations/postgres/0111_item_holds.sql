-- 0111: a ticket marked Not finished (Arugula #841). See SQLite's file, in
-- Postgres's words.
CREATE TABLE item_holds (
    project_id TEXT COLLATE pg_c_utf8 NOT NULL,
    item TEXT COLLATE pg_c_utf8 NOT NULL,
    reason TEXT COLLATE pg_c_utf8 NOT NULL,
    held_by TEXT COLLATE pg_c_utf8 NOT NULL,
    held_by_agent TEXT COLLATE pg_c_utf8,
    held_at BIGINT NOT NULL,
    skipped TEXT COLLATE pg_c_utf8,
    PRIMARY KEY (project_id, item)
);
