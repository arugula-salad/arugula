-- 0034: deploy artifacts and shipped apps' names (M233, platform step 8): the source an agent
-- shipped through planter's Ship driver, kept until a deploy's run takes it.
-- Times are milliseconds since the epoch. `db::deploys` has the rules.

-- A gzipped tar an agent (or a person) shipped to one of a project's
-- planter-ship environments, by its digest (`sha256:` and 64 hex): the
-- deploy's source names it, the plan's digest binds it, and the deploy's
-- box checks the bytes against it. Kept per project, so a project's
-- deploys read only its own; a ship of the same bytes again is the same
-- row. `created_by` is the account that shipped it (an agent's owner).
-- It's kept only after its deploy is planned, and goes once no deploy that
-- names it is waiting or running; a project keeps at most 200 MiB.
CREATE TABLE deploy_artifacts (
    project_id TEXT NOT NULL,
    digest TEXT NOT NULL,
    bytes BLOB NOT NULL,
    size INTEGER NOT NULL,
    created_by TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    PRIMARY KEY (project_id, digest)
);
CREATE INDEX deploy_artifacts_at ON deploy_artifacts (project_id, created_at);

-- A shipped app's name, claimed by the project whose planter-ship or
-- planter-promote environment named it first (M233's review): its primary
-- key makes the claim atomic, so two projects can't both have a name. A
-- deleted project's claims go with it.
CREATE TABLE planter_app_claims (
    app TEXT PRIMARY KEY,
    project_id TEXT NOT NULL,
    created_at INTEGER NOT NULL
);
