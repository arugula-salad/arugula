-- 0109: `docs.write` for every machine that holds a live `issues.write`
-- over a project (Arugula #862). See SQLite's file, in Postgres's words.
INSERT INTO grants (id, subject_kind, subject_id, power, scope_kind, scope_id, granted_by, created_at)
SELECT 'grt_' || md5(random()::text || clock_timestamp()::text || g.id), g.subject_kind, g.subject_id, 'docs.write',
       g.scope_kind, g.scope_id, g.granted_by, (extract(epoch FROM clock_timestamp()) * 1000)::bigint
FROM grants g
WHERE g.power = 'issues.write' AND g.subject_kind = 'daemon' AND g.scope_kind = 'project' AND g.revoked_at IS NULL
  AND NOT EXISTS (SELECT 1 FROM grants d WHERE d.subject_kind = g.subject_kind AND d.subject_id = g.subject_id
      AND d.power = 'docs.write' AND d.scope_kind = g.scope_kind AND d.scope_id = g.scope_id AND d.revoked_at IS NULL);
