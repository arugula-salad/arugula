-- 0082: what agents may write to a Plane project, per kind of write, and what
-- a write's footer says (Arugula #423). `write_policy` is JSON, one of
-- `allow` or `approve` for each of create, comment, edit, state and fields;
-- `attribution` is `full`, `short` or `none`. A connection made from now on
-- gets the middle preset and the short footer. A connection that exists keeps
-- what it had: `allow` becomes every kind allowed, `approve` every kind
-- approved, so an upgrade never loosens one. `agent_writes` stays in the
-- table, no longer read.
-- Additive: the build before this one ignores the columns.
ALTER TABLE tracker_connections ADD COLUMN write_policy TEXT NOT NULL DEFAULT '{"create":"approve","comment":"allow","edit":"allow","state":"approve","fields":"allow"}';
ALTER TABLE tracker_connections ADD COLUMN attribution TEXT NOT NULL DEFAULT 'short' CHECK (attribution IN ('full', 'short', 'none'));
UPDATE tracker_connections SET write_policy = '{"create":"allow","comment":"allow","edit":"allow","state":"allow","fields":"allow"}' WHERE agent_writes = 'allow';
UPDATE tracker_connections SET write_policy = '{"create":"approve","comment":"approve","edit":"approve","state":"approve","fields":"approve"}' WHERE agent_writes = 'approve';
