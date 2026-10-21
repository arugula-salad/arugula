-- 0003: M129's backfill, once more and for the last time. It used to run on
-- every start: every account with a Cloud workspace or a cloud secret that
-- has no project gets its personal project, and those rows get it.
-- Production ran it on every start since M129, so there it finds nothing;
-- a new database has nothing to find. New rows get their project when
-- they're made. Team channels wait for their team's name from the hub
-- (`projects::start`), so they're not done here.
INSERT INTO projects (id, account_id, name, is_default, created_at)
SELECT 'prj_' || lower(hex(randomblob(16))), o.account, 'Personal', 1, CAST(unixepoch('subsec') * 1000 AS INTEGER)
FROM (SELECT account FROM cloud_workspaces WHERE project_id IS NULL
      UNION SELECT account FROM cloud_secrets WHERE project_id IS NULL) o
WHERE NOT EXISTS (SELECT 1 FROM projects p WHERE p.account_id = o.account AND p.is_default = 1);

UPDATE cloud_workspaces SET project_id = (SELECT p.id FROM projects p
    WHERE p.account_id = cloud_workspaces.account AND p.is_default = 1)
WHERE project_id IS NULL;

UPDATE cloud_secrets SET project_id = (SELECT p.id FROM projects p
    WHERE p.account_id = cloud_secrets.account AND p.is_default = 1)
WHERE project_id IS NULL;
