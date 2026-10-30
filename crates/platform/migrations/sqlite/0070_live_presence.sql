-- 0070: live presence (Arugula #341, the `live-presence` Labs flag): the
-- account's choice, `off` (the default) or `watching`, kept beside chat's
-- presence mode so every page agrees. Live (the camera) is per window and
-- isn't kept.
--
-- Additive: the build before this one doesn't read the column, so control
-- can still roll back past it.
ALTER TABLE chat_presence ADD COLUMN live TEXT NOT NULL DEFAULT 'off' CHECK (live IN ('off', 'watching'));
