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
ALTER TABLE hosted_workspaces ADD COLUMN setup_status TEXT COLLATE pg_c_utf8 CHECK (setup_status IN ('setting_up', 'ready', 'failed'));
ALTER TABLE hosted_workspaces ADD COLUMN setup_step TEXT COLLATE pg_c_utf8;
ALTER TABLE hosted_workspaces ADD COLUMN setup_reason TEXT COLLATE pg_c_utf8;
ALTER TABLE hosted_workspaces ADD COLUMN setup_pane BIGINT;
ALTER TABLE hosted_workspaces ADD COLUMN setup_at BIGINT;
