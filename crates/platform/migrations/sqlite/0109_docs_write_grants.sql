-- 0109: `docs.write`, an agent's power to make and write a project's Docs
-- (Arugula #862). A team's Docs live in its project, and a machine's agents
-- write them there as their owner's, so every machine that may write the
-- project's issues (`issues.write`, live, over a project) is given it too,
-- by the account that gave the other: today's team machines keep working.
-- A token's grants are left alone: a token has no Docs tool.
INSERT INTO grants (id, subject_kind, subject_id, power, scope_kind, scope_id, granted_by, created_at)
SELECT 'grt_' || lower(hex(randomblob(16))), g.subject_kind, g.subject_id, 'docs.write', g.scope_kind, g.scope_id,
       g.granted_by, CAST(unixepoch('subsec') * 1000 AS INTEGER)
FROM grants g
WHERE g.power = 'issues.write' AND g.subject_kind = 'daemon' AND g.scope_kind = 'project' AND g.revoked_at IS NULL
  AND NOT EXISTS (SELECT 1 FROM grants d WHERE d.subject_kind = g.subject_kind AND d.subject_id = g.subject_id
      AND d.power = 'docs.write' AND d.scope_kind = g.scope_kind AND d.scope_id = g.scope_id AND d.revoked_at IS NULL);
