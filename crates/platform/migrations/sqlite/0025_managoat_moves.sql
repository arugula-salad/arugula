-- 0025: moving beta Cloud workspaces off Managoat (M189, platform step 5,
-- #1235). "Move to the new Cloud workspaces" recreates a Managoat-backed
-- Cloud workspace as a hosted workspace in the same project, from its
-- spec, with its secrets and its tabs' transcripts. The Managoat one stays
-- until it's deleted; these say where it went. Both go with
-- `cloud_workspaces`, after the deletion.
--
-- - `moved_to`: the hosted workspace (`hw_`) it's moving or moved to, kept
--   before anything is made, so a move asked again makes no second one.
-- - `moved_at`: when the move finished (ms); NULL while it's under way.
ALTER TABLE cloud_workspaces ADD COLUMN moved_to TEXT;
ALTER TABLE cloud_workspaces ADD COLUMN moved_at INTEGER;
