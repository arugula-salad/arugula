-- 0024: public previews (M187, platform step 5, #1235). A port a member
-- made public in a hosted workspace is served to anyone at
-- `<public_id>.<preview domain>`, through control (aru-097).
--
-- `public_id` is the preview's host label: 32 random hex characters, not
-- the workspace's id, made fresh each time the port is made public and
-- kept while it's renewed. It's NULL while the port isn't public, as are
-- `public_by` (the account that made it public or last renewed it) and
-- `public_until` (when it stops, 7 days after that unless renewed).
ALTER TABLE hosted_ports ADD COLUMN public_id TEXT COLLATE pg_c_utf8;
CREATE UNIQUE INDEX hosted_ports_public ON hosted_ports (public_id) WHERE public_id IS NOT NULL;
