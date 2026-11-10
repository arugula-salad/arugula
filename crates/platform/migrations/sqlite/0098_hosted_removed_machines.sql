-- 0098: a hosted machine the hub says it removed (Arugula #744): an owner
-- did, at control, and the hub told the platform (`PlatformPort::hosted_removed`).
-- One row per workspace, while its worker hasn't failed it yet: written
-- by `hosted::machine_removed`, read by the worker at its next step, and
-- gone in the checkpoint that fails the workspace, or that finds it with
-- another machine (a rebuild) or none. `at` is milliseconds since the epoch.
CREATE TABLE hosted_removed_machines (
    workspace TEXT PRIMARY KEY,
    machine TEXT NOT NULL,
    at INTEGER NOT NULL
);
