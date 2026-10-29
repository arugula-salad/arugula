-- 0067: a board is an epic (boards, Arugula #301, aru-122). A board gains an
-- optional goal and, once closed, a done note with when and by whom.
-- `closed_at` is NULL while it is open; `done_note` is kept when it is
-- opened again.
--
-- Additive: the build before this one doesn't read these columns, so control
-- can still roll back past it.
ALTER TABLE boards ADD COLUMN goal TEXT NOT NULL DEFAULT '';
ALTER TABLE boards ADD COLUMN done_note TEXT NOT NULL DEFAULT '';
ALTER TABLE boards ADD COLUMN closed_at BIGINT;
ALTER TABLE boards ADD COLUMN closed_by TEXT NOT NULL DEFAULT '';
