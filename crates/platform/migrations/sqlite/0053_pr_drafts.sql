-- 0053: an agent session's pull request, as its asker drafts and sends it
-- (M259, thread = session, Arugula #129). Times are milliseconds since the
-- epoch. `db::pr_drafts` has the rules, and `sessions::pr` the routes.
--
-- One per session (`id` is `pdr_` and 32 hex characters): the PR from
-- the session's branch (`head`, its `arugula/<8 hex>-<slug>`) into `base`
-- on the linked repo `repo_id`, with the `title` and `body` its asker
-- wrote (prefilled from the repo's PR template and the session's turns),
-- opened on GitHub as a draft PR when `draft`. `author` is the session's
-- asker: only they send it, as themselves (aru-041). `state` is
-- `waiting`, `sent` or `failed`; a send holds it while it goes by setting
-- `sent_at` (as `drafts` does), which a sent one keeps. `number` and
-- `sent_url` are the PR's once sent, and `noted_at` when the PR's thread
-- (its `fi_` item's) got the line that links back to the session.
CREATE TABLE pr_drafts (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL UNIQUE,
    project_id TEXT NOT NULL,
    author TEXT NOT NULL,
    repo_id INTEGER NOT NULL,
    head TEXT NOT NULL,
    base TEXT NOT NULL,
    title TEXT NOT NULL,
    body TEXT NOT NULL DEFAULT '',
    draft INTEGER NOT NULL DEFAULT 1,
    state TEXT NOT NULL DEFAULT 'waiting' CHECK (state IN ('waiting', 'sent', 'failed')),
    sent_at INTEGER,
    sent_url TEXT,
    number INTEGER,
    error TEXT,
    noted_at INTEGER,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE INDEX pr_drafts_author ON pr_drafts (author);
CREATE INDEX pr_drafts_unnoted ON pr_drafts (repo_id, number) WHERE state = 'sent' AND noted_at IS NULL;
