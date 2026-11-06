-- 0103: a relation kind `before` (Arugula #761): the first item comes
-- before the second, in the order the work is done in. Like `blocks` it
-- can't close a cycle; unlike it, it doesn't gate a row's being ready, and
-- that is a property of the kind, in code. SQLite changes a `CHECK` by making
-- the table again, with every column it has now (0077).
-- Additive: the build before this one reads a `before` edge as `relates`.
CREATE TABLE work_relations_new (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL,
    from_kind TEXT NOT NULL,
    from_id TEXT NOT NULL,
    to_kind TEXT NOT NULL,
    to_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    source TEXT NOT NULL,
    created_by TEXT NOT NULL DEFAULT '',
    created_by_agent TEXT,
    created_at INTEGER NOT NULL,
    CHECK (kind IN ('blocks', 'relates', 'duplicate_of', 'discovered_from', 'before')),
    CHECK (source IN ('person', 'agent', 'plane', 'github')),
    CHECK (from_kind IN ('issue', 'forge_item') AND to_kind IN ('issue', 'forge_item'))
);
INSERT INTO work_relations_new (id, project_id, from_kind, from_id, to_kind, to_id, kind, source,
        created_by, created_by_agent, created_at)
    SELECT id, project_id, from_kind, from_id, to_kind, to_id, kind, source,
        created_by, created_by_agent, created_at
    FROM work_relations;
DROP TABLE work_relations;
ALTER TABLE work_relations_new RENAME TO work_relations;
CREATE UNIQUE INDEX work_relations_edge ON work_relations (from_id, to_id, kind);
CREATE INDEX work_relations_to ON work_relations (to_id, kind);
CREATE INDEX work_relations_project ON work_relations (project_id);
