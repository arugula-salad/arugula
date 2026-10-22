-- 0015: the platform's one leased job table (M208, platform step 7).
-- Workspaces', runs', deploys' and schedules' background work, each a job
-- the platform's machines take turns at (`crate::jobs`), as `cloud_jobs`
-- does for Cloud runs, workspaces and tabs until M190 moves those here.
--
-- - `id` is the thing's own (a run's, a deploy's), one job for its whole
--   life. `kind` says which handler steps it: `workspace`, `run`, `deploy`
--   or `schedule`.
-- - `payload` is what it was asked (JSON), set when it's queued.
--   `checkpoint` is how far it got (JSON), written only by the machine
--   holding its lease, so a machine that takes it over carries on from
--   there.
-- - It's due when `state` is `pending`, `next_at` has passed and nobody
--   holds it: `lease_until` has passed. A claim writes a new `lease`
--   (random) and `lease_until`; a write that names an older `lease` is
--   refused. `cancel` is asking it to stop; its handler ends it.
-- - `attempts` counts claims since it last handed itself back cleanly, for
--   the back-off after a failure; `error` is the last failure's.
CREATE TABLE platform_jobs (
    id TEXT PRIMARY KEY,
    kind TEXT NOT NULL,
    payload TEXT NOT NULL,
    checkpoint TEXT,
    state TEXT NOT NULL DEFAULT 'pending' CHECK (state IN ('pending', 'done', 'failed', 'cancelled')),
    cancel INTEGER NOT NULL DEFAULT 0,
    attempts INTEGER NOT NULL DEFAULT 0,
    error TEXT,
    next_at INTEGER NOT NULL DEFAULT 0,
    lease TEXT,
    lease_until INTEGER NOT NULL DEFAULT 0,
    created INTEGER NOT NULL,
    updated INTEGER NOT NULL
);
CREATE INDEX platform_jobs_due ON platform_jobs (next_at, created) WHERE state = 'pending';
