-- 0081: what a Plane project offers a work item (Arugula #422): its states,
-- modules, types and labels, kept as JSON beside the connection so a write
-- can name them without a read of Plane each time. Refreshed at reconcile,
-- and once on demand when a write names something not in it.
-- Additive: the build before this one ignores the columns.
ALTER TABLE tracker_connections ADD COLUMN fields TEXT COLLATE pg_c_utf8;
ALTER TABLE tracker_connections ADD COLUMN fields_at BIGINT;
