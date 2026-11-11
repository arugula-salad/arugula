-- 0106: close-on-merge says what closed the ticket, and can be switched off
-- (Arugula #852). A connection has a switch, `close_on_merge`, on by
-- default as before: while it is on, a merged PR that names a Plane item
-- moves it to Done. `tracker_merge_closes` has a row for each item Arugula
-- moved that way, so a later merge never closes one a person has moved out
-- of Done since.
-- Additive: the build before this one ignores the column and the table.
ALTER TABLE tracker_connections ADD COLUMN close_on_merge BOOLEAN NOT NULL DEFAULT TRUE;
CREATE TABLE tracker_merge_closes (
    connection_id TEXT COLLATE pg_c_utf8 NOT NULL,
    number BIGINT NOT NULL,
    closed_at BIGINT NOT NULL,
    PRIMARY KEY (connection_id, number)
);
