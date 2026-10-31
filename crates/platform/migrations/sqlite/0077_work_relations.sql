-- 0077: relations between work items (Arugula #403). A typed edge from one
-- item of a project to another: each end is a pointer, `issue` (an `iss_`
-- id) or `forge_item` (an `fi_` id: a mirrored GitHub or tracker item).
-- `kind` is `blocks`, `relates`, `duplicate_of` or `discovered_from`; only
-- `blocks` gates a row's being ready, and that is a property of the kind,
-- in code, not stored. `source` says who made the edge: a `person`, an
-- `agent`, or `plane` / `github` for an edge read from the tracker, which
-- Arugula doesn't change. `created_by` is an account ('' for a mirrored
-- edge, or once the account is deleted) and `created_by_agent` the machine
-- whose agent made it, NULL for a person's or a mirrored edge. Times are milliseconds since the
-- epoch. The same edge is stored once.
-- Additive: the build before this one doesn't know the table.
CREATE TABLE work_relations (
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
    CHECK (kind IN ('blocks', 'relates', 'duplicate_of', 'discovered_from')),
    CHECK (source IN ('person', 'agent', 'plane', 'github')),
    CHECK (from_kind IN ('issue', 'forge_item') AND to_kind IN ('issue', 'forge_item'))
);
CREATE UNIQUE INDEX work_relations_edge ON work_relations (from_id, to_id, kind);
CREATE INDEX work_relations_to ON work_relations (to_id, kind);
CREATE INDEX work_relations_project ON work_relations (project_id);
