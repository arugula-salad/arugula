-- 0094: a session leads a board (Arugula #414). A post on the board's
-- thread reaches the session that leads it, wherever it runs.
-- `lead_machine` is the machine's id, `lead_session` the session's name as
-- that machine's daemon matches it, and `lead_at` the time it took the lead,
-- in milliseconds since the epoch. All NULL for a board nobody leads.
--
-- Additive: the build before this one ignores the columns, and a board
-- shows no lead.
ALTER TABLE boards ADD COLUMN lead_machine TEXT COLLATE pg_c_utf8;
ALTER TABLE boards ADD COLUMN lead_session TEXT COLLATE pg_c_utf8;
ALTER TABLE boards ADD COLUMN lead_at BIGINT;
