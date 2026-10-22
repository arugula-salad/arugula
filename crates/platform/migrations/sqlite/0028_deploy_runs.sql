-- 0028: deploys' runs (M219, platform step 7). The run that applies an
-- approved deploy, on its project's hosted workspace (`db::deploy_runs`).
-- Times are milliseconds since the epoch.

-- One per deploy, made when its approval is applied: `run_id` is the run
-- (`runs`, M210) whose operation is the driver's apply of this deploy
-- (`kind deploy`), the only run whose `started` the deploy's log takes.
-- `approval_id` is the approval it applies (`deploy_approvals`);
-- `workspace_id` the hosted workspace it runs on; `driver` the
-- environment's driver then. `secrets_at` is when the environment's
-- secrets were given to the run's machine: once, while it ran, never
-- again. `paged_at` is when the project's owners were told its apply
-- failed: once.
CREATE TABLE deploy_runs (
    deploy_id TEXT PRIMARY KEY,
    run_id TEXT NOT NULL UNIQUE,
    approval_id TEXT NOT NULL,
    workspace_id TEXT NOT NULL,
    driver TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    secrets_at INTEGER,
    paged_at INTEGER
);

-- A hosted workspace's purpose: `workspace` (people's, M179) or
-- `deploy`, a box made for one deploy's run and deleted after it (M219):
-- never listed to people, never opened, not counted in a project's limit,
-- and metered apart (M188).
ALTER TABLE hosted_workspaces ADD COLUMN purpose TEXT NOT NULL DEFAULT 'workspace'
    CHECK (purpose IN ('workspace', 'deploy'));

-- An approval used up (M219): set in the transaction that records its
-- deploy's `started`, by the run that started it. A used approval never
-- starts that deploy again, nor goes to another box.
ALTER TABLE deploy_approvals ADD COLUMN consumed_at INTEGER;
ALTER TABLE deploy_approvals ADD COLUMN consumed_run TEXT;
