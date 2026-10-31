-- 0078: when a Plane webhook last arrived (Arugula #417). Set on a delivery
-- whose signature checked, for a connection's row in project settings.
-- Additive: the build before this one ignores the column.
ALTER TABLE tracker_connections ADD COLUMN last_delivery_at BIGINT;
