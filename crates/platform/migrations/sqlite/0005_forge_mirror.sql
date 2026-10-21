-- 0005: the forge mirror's tables (M162, platform step 4, #1204). Linked
-- repos' PRs and issues as platform rows, kept current by the GitHub App's
-- webhooks and one leased poller per repo. Nothing writes them yet: M161
-- (deliveries), M163 (installations, links), M164 (items, checks, events),
-- M166 (content), M167 (mints), M170 (drafts) and M171 (inbox) do.
--
-- GitHub's own ids are its numbers (installation, account, repo, comment):
-- a rename or a new login keeps them. Times are milliseconds since the
-- epoch, GitHub's as well as ours. JSON lists and objects are text.

-- What the App's webhook received, by `X-GitHub-Delivery`, so a
-- redelivery is recognised after a restart (M161). Kept 7 days.
CREATE TABLE github_webhook_deliveries (
    delivery TEXT PRIMARY KEY,
    event TEXT NOT NULL,
    received_at INTEGER NOT NULL
);
CREATE INDEX github_webhook_deliveries_received ON github_webhook_deliveries (received_at);

-- Where the App is installed: a GitHub user or organization, what it was
-- granted, and whether it's suspended or gone.
CREATE TABLE github_installations (
    installation_id INTEGER PRIMARY KEY,
    github_account_id INTEGER NOT NULL,
    github_login TEXT NOT NULL,
    github_account_type TEXT NOT NULL,
    permissions TEXT NOT NULL DEFAULT '{}',
    suspended_at INTEGER,
    deleted_at INTEGER,
    updated_at INTEGER NOT NULL
);
CREATE INDEX github_installations_account ON github_installations (github_account_id);

-- A repo linked to a project (M163), by GitHub's repo id. `full_name` is
-- kept current on a rename. `linked_by` is the account that linked it,
-- NULL once that account is deleted. An unlinked repo keeps its row until
-- `purge_after`, when its rows go (M163's purge job). `lease` and
-- `lease_until` are its poller's (M164, as `cloud_jobs`'), and
-- `backfilled_at` and `reconciled_at` when that last finished each.
CREATE TABLE linked_repos (
    repo_id INTEGER PRIMARY KEY,
    full_name TEXT NOT NULL,
    private INTEGER NOT NULL DEFAULT 0,
    project_id TEXT NOT NULL,
    installation_id INTEGER NOT NULL,
    linked_by TEXT,
    linked_at INTEGER NOT NULL,
    mirror_content INTEGER NOT NULL DEFAULT 0,
    unlinked_at INTEGER,
    purge_after INTEGER,
    lease TEXT,
    lease_until INTEGER NOT NULL DEFAULT 0,
    backfilled_at INTEGER,
    reconciled_at INTEGER
);
CREATE INDEX linked_repos_project ON linked_repos (project_id);
CREATE INDEX linked_repos_installation ON linked_repos (installation_id);
CREATE INDEX linked_repos_purge ON linked_repos (purge_after) WHERE purge_after IS NOT NULL;
CREATE INDEX linked_repos_full_name ON linked_repos (full_name);

-- A PR or an issue, as metadata: never its body (that's `forge_content`,
-- for projects that opted in). `id` is the platform's (`fi_` and 32 hex
-- characters). `kind` is `pr` or `issue`; `state` `open`, `closed` or
-- `merged`. `labels`, `assignees` and `reviewers` (requested) are JSON
-- lists. `created_at` and `updated_at` are GitHub's, `synced_at` when this
-- row last changed from it.
CREATE TABLE forge_items (
    id TEXT PRIMARY KEY,
    repo_id INTEGER NOT NULL,
    number INTEGER NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('pr', 'issue')),
    state TEXT NOT NULL,
    title TEXT NOT NULL,
    labels TEXT NOT NULL DEFAULT '[]',
    author_login TEXT,
    author_id INTEGER,
    assignees TEXT NOT NULL DEFAULT '[]',
    reviewers TEXT NOT NULL DEFAULT '[]',
    draft INTEGER NOT NULL DEFAULT 0,
    merge_state TEXT,
    head_ref TEXT,
    head_sha TEXT,
    base_ref TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    synced_at INTEGER NOT NULL,
    UNIQUE (repo_id, number)
);
CREATE INDEX forge_items_updated ON forge_items (repo_id, updated_at);
CREATE INDEX forge_items_head ON forge_items (repo_id, head_sha);

-- A head SHA's check runs and commit statuses, one row each by name.
-- `kind` is `check_run` or `status`.
CREATE TABLE forge_checks (
    repo_id INTEGER NOT NULL,
    head_sha TEXT NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('check_run', 'status')),
    name TEXT NOT NULL,
    status TEXT NOT NULL,
    conclusion TEXT,
    url TEXT,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (repo_id, head_sha, kind, name)
);

-- What happened to an item, as metadata: a review (and its state), a
-- review request, a state change. `id` is the platform's (`fev_`);
-- `github_id`, when GitHub gives one, keeps a redelivered or reconciled
-- event from landing twice.
CREATE TABLE forge_events (
    id TEXT PRIMARY KEY,
    repo_id INTEGER NOT NULL,
    number INTEGER NOT NULL,
    kind TEXT NOT NULL,
    github_id INTEGER,
    actor_login TEXT,
    actor_id INTEGER,
    review_state TEXT,
    at INTEGER NOT NULL
);
CREATE INDEX forge_events_item ON forge_events (repo_id, number, at);
CREATE UNIQUE INDEX forge_events_github ON forge_events (repo_id, kind, github_id) WHERE github_id IS NOT NULL;

-- Bodies and comments, only for a repo whose project opted in
-- (`linked_repos.mirror_content`, M166). `kind` is `body`, `comment` or
-- `review_comment`; `github_id` is the item's for a body, else the
-- comment's.
CREATE TABLE forge_content (
    repo_id INTEGER NOT NULL,
    number INTEGER NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('body', 'comment', 'review_comment')),
    github_id INTEGER NOT NULL,
    author_login TEXT,
    author_id INTEGER,
    body TEXT NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (repo_id, kind, github_id)
);
CREATE INDEX forge_content_item ON forge_content (repo_id, number);

-- A comment, review or issue someone (or their agent) wrote and a person
-- sends (M168, M170). `id` is the platform's (`dft_`). An agent's draft
-- names the agent and its machine; `author` is the person it's for.
-- `number` is NULL for a new issue. `event` is a review's (`COMMENT`,
-- `APPROVE`, `REQUEST_CHANGES`), `title` an issue's. `state` is
-- `waiting`, `sent`, `discarded` or `failed`. `import_key` is a daemon's
-- own id for a draft it uploaded (M170), so an upload lands once.
CREATE TABLE drafts (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL,
    author TEXT NOT NULL,
    agent_name TEXT,
    agent_machine TEXT,
    repo_id INTEGER NOT NULL,
    number INTEGER,
    kind TEXT NOT NULL CHECK (kind IN ('comment', 'review', 'issue')),
    event TEXT,
    title TEXT,
    body TEXT NOT NULL,
    state TEXT NOT NULL DEFAULT 'waiting' CHECK (state IN ('waiting', 'sent', 'discarded', 'failed')),
    sent_at INTEGER,
    sent_url TEXT,
    error TEXT,
    import_key TEXT UNIQUE,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE INDEX drafts_author ON drafts (author, state);
CREATE INDEX drafts_target ON drafts (repo_id, number);
CREATE INDEX drafts_project ON drafts (project_id);

-- A member's inbox (M171): an item (`forge_items.id`) and why it's there
-- (a review requested, a mention, checks failed ...), and when it was
-- seen and done.
CREATE TABLE inbox_items (
    account TEXT NOT NULL,
    item TEXT NOT NULL,
    reason TEXT NOT NULL,
    seen_at INTEGER,
    done_at INTEGER,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (account, item, reason)
);
CREATE INDEX inbox_items_item ON inbox_items (item);

-- Every GitHub token the platform minted (M167): `kind` is
-- `installation` or `user`, `permissions` the JSON it was scoped to,
-- `caller` the account it was for or `platform`, `purpose` what for.
-- Never the token. `id` is the platform's (`gtm_`).
CREATE TABLE github_token_mints (
    id TEXT PRIMARY KEY,
    kind TEXT NOT NULL CHECK (kind IN ('installation', 'user')),
    installation_id INTEGER,
    repo_id INTEGER,
    permissions TEXT NOT NULL DEFAULT '{}',
    caller TEXT NOT NULL,
    purpose TEXT NOT NULL,
    at INTEGER NOT NULL
);
CREATE INDEX github_token_mints_at ON github_token_mints (at);
CREATE INDEX github_token_mints_repo ON github_token_mints (repo_id, at);
