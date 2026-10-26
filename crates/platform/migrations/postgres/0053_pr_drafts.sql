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
    id TEXT COLLATE pg_c_utf8 PRIMARY KEY,
    session_id TEXT COLLATE pg_c_utf8 NOT NULL UNIQUE,
    project_id TEXT COLLATE pg_c_utf8 NOT NULL,
    author TEXT COLLATE pg_c_utf8 NOT NULL,
    repo_id BIGINT NOT NULL,
    head TEXT COLLATE pg_c_utf8 NOT NULL,
    base TEXT COLLATE pg_c_utf8 NOT NULL,
    title TEXT COLLATE pg_c_utf8 NOT NULL,
    body TEXT COLLATE pg_c_utf8 NOT NULL DEFAULT '',
    draft BOOLEAN NOT NULL DEFAULT TRUE,
    state TEXT COLLATE pg_c_utf8 NOT NULL DEFAULT 'waiting' CHECK (state IN ('waiting', 'sent', 'failed')),
    sent_at BIGINT,
    sent_url TEXT COLLATE pg_c_utf8,
    number BIGINT,
    error TEXT COLLATE pg_c_utf8,
    noted_at BIGINT,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL
);
CREATE INDEX pr_drafts_author ON pr_drafts (author);
CREATE INDEX pr_drafts_unnoted ON pr_drafts (repo_id, number) WHERE state = 'sent' AND noted_at IS NULL;
