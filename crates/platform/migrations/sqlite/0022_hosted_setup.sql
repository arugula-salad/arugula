-- 0022: a hosted workspace's setup (M183, platform step 5, #1235). On its
-- box's first boot, its daemon clones the spec's repos, installs its
-- packages and runs its setup script in a pane anyone on the project can
-- watch, and says how it goes. A rebuild clears it.
--
-- - `setup_status`: `setting_up`, `ready` or `failed`; NULL before the
--   box's daemon says anything.
-- - `setup_step`: the step it's on, or the one that failed.
-- - `setup_reason`: why it failed, a fixed code (`exit`, `pane_closed`,
--   `start`). Never the pane's text: that stays end to end, and a client
--   reads it from the box.
-- - `setup_pane`: the pane it runs in, on the box.
-- - `setup_at`: when the daemon last said (ms).
ALTER TABLE hosted_workspaces ADD COLUMN setup_status TEXT CHECK (setup_status IN ('setting_up', 'ready', 'failed'));
ALTER TABLE hosted_workspaces ADD COLUMN setup_step TEXT;
ALTER TABLE hosted_workspaces ADD COLUMN setup_reason TEXT;
ALTER TABLE hosted_workspaces ADD COLUMN setup_pane INTEGER;
ALTER TABLE hosted_workspaces ADD COLUMN setup_at INTEGER;
