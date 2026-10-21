-- 0006: what GitHub said about a person's access to a private linked repo
-- (M165, #1211), kept 10 minutes, and what drops those answers early, so
-- every machine of the platform agrees: a webhook taken on one machine cuts
-- a removed collaborator off on all of them.
--
-- `github_id` is the person's numeric GitHub id, `owner` the repo's owner's
-- login in lower case, `asked_at` when GitHub was asked (before the
-- answer came, so a drop while it was asking wins).
CREATE TABLE forge_access (
    github_id BIGINT NOT NULL,
    repo_id BIGINT NOT NULL,
    owner TEXT COLLATE pg_c_utf8 NOT NULL,
    allowed BOOLEAN NOT NULL,
    asked_at BIGINT NOT NULL,
    PRIMARY KEY (github_id, repo_id)
);
CREATE INDEX forge_access_asked ON forge_access (asked_at);

-- A `member`, `membership`, `organization` or `repository` event (and the
-- App's installation and team events) drops every answer asked before it
-- for its `scope`: `repo` (GitHub's repo id), `owner` (a login in lower
-- case) or `person` (a GitHub id). Kept as long as an answer is.
CREATE TABLE forge_access_drops (
    scope TEXT COLLATE pg_c_utf8 NOT NULL CHECK (scope IN ('repo', 'owner', 'person')),
    subject TEXT COLLATE pg_c_utf8 NOT NULL,
    at BIGINT NOT NULL,
    PRIMARY KEY (scope, subject)
);
