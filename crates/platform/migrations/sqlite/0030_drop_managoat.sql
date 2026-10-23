-- 0030: the Managoat-era tables go (M189 and M190, platform step 5, #1235).
-- Cloud workspaces are hosted workspaces now (`hosted_workspaces`, M179);
-- the Managoat-backed ones, Cloud runs (S40) and the agent library were
-- removed, and Managoat's own resources were deleted first. Nothing is
-- archived: their only user asked for everything to go (2026-10-23).
--
-- Not additive: a build from before it reads these tables, so it can't
-- start on a database that ran this (`migrate`'s module docs).
--
-- Children first: `cloud_tabs` points at `cloud_workspaces`, and
-- `cloud_run_secrets` at `cloud_runs`.
DROP TABLE cloud_tabs;
DROP TABLE cloud_workspace_specs;
DROP TABLE cloud_workspaces;
DROP TABLE cloud_events;
DROP TABLE cloud_notifications;
DROP TABLE cloud_requests;
DROP TABLE cloud_jobs;
DROP TABLE cloud_run_secrets;
DROP TABLE cloud_runs;
DROP TABLE cloud_secrets;
DROP TABLE cloud_agents;
