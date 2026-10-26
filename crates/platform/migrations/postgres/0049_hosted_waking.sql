-- 0049: a hosted workspace can be `waking` (M258, Arugula #124): its box resumed,
-- its daemon not yet dialed in again. It's `ready` only once it has; one
-- whose daemon doesn't within the platform's dial wait is `failed`, its
-- `error` saying it needs a rebuild.
--
-- Additive: it only lets `state` hold one more value. A build from before
-- this one claims a `waking` row like a `ready` one, reads its box's state
-- and calls it `ready`, as it did before. Postgres changes the `CHECK` in
-- place (0013 left it its default name).
ALTER TABLE hosted_workspaces DROP CONSTRAINT hosted_workspaces_state_check;
ALTER TABLE hosted_workspaces ADD CONSTRAINT hosted_workspaces_state_check
    CHECK (state IN ('creating', 'enrolling', 'ready', 'waking', 'paused', 'failed', 'deleting', 'deleted'));
