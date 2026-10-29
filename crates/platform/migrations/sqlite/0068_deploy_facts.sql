-- 0068: what a deploy is, as the platform read it (deploys, Arugula #333).
-- When a commit's deploy is planned the platform reads its subject (the
-- commit's first line) from GitHub, and for an arugula-release environment
-- works out the version the run is expected to tag. Both are NULL when
-- GitHub didn't answer, and for an artifact. They are shown beside the
-- deploy's signed facts and are in none of them: not the plan, the
-- operation or an event.
--
-- Additive: the build before this one doesn't read these columns, so control
-- can still roll back past it.
ALTER TABLE deploys ADD COLUMN subject TEXT;
ALTER TABLE deploys ADD COLUMN expected_version TEXT;
