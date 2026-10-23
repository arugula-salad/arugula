-- 0035: planter's import (M232, platform step 8): which of the repos
-- planter's boxes were planted from the import linked to a project, so a
-- second run changes nothing and a repo unlinked since is never linked
-- again by it. Times are milliseconds since the epoch. `crate::planter`
-- has the rules.

-- One planted repo, by its planter (`planter_user`: planter's own id, the
-- person's numeric GitHub id) and the repo as planter named it, lower
-- case. `repo_id` is GitHub's, `project_id` where it went, `account_id`
-- the platform account it was imported for (the planter, linked on the
-- hub). `outcome` is `linked` (the import linked it) or `already` (it was
-- linked there by hand first). Imported by the operator; never updated.
CREATE TABLE planter_imports (
    planter_user TEXT NOT NULL,
    repo TEXT NOT NULL,
    repo_id INTEGER NOT NULL,
    project_id TEXT NOT NULL,
    account_id TEXT NOT NULL,
    outcome TEXT NOT NULL CHECK (outcome IN ('linked', 'already')),
    imported_at INTEGER NOT NULL,
    PRIMARY KEY (planter_user, repo)
);
CREATE INDEX planter_imports_account ON planter_imports (account_id);
CREATE INDEX planter_imports_repo ON planter_imports (repo_id);
