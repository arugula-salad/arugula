-- 0069: a board made or closed by an agent (boards, Arugula #335, aru-123).
-- `created_by_agent` and `closed_by_agent` are the machine whose agent made
-- or closed the board under its `boards.write` grant; NULL when a person did.
-- `created_by` and `closed_by` stay the person's account: the owner the
-- agent acted for.
--
-- Additive: the build before this one doesn't read these columns, so control
-- can still roll back past it.
ALTER TABLE boards ADD COLUMN created_by_agent TEXT COLLATE pg_c_utf8;
ALTER TABLE boards ADD COLUMN closed_by_agent TEXT COLLATE pg_c_utf8;
