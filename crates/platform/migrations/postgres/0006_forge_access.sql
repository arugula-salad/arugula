-- 0006: what GitHub said about a person's access to a repository (M165,
-- #1211), kept 10 minutes, and what drops those answers early, so every
-- machine of the platform agrees: a webhook taken on one machine cuts a
-- removed collaborator off on all of them. The viewer check, daemons'
-- live updates and hosted boxes' tokens all ask through these.
--
-- `github_id` is the person's numeric GitHub id, `repo` the repository's
-- `owner/name` and `owner` its owner's login, both in lower case. `why` is
-- why not, NULL when they have access. `asked_at` is when GitHub was
-- asked, by the database's clock (before the answer came, so a drop while
-- it was asking wins).
CREATE TABLE forge_access (
    github_id BIGINT NOT NULL,
    repo TEXT COLLATE pg_c_utf8 NOT NULL,
    owner TEXT COLLATE pg_c_utf8 NOT NULL,
    why TEXT COLLATE pg_c_utf8,
    asked_at BIGINT NOT NULL,
    PRIMARY KEY (github_id, repo)
);
CREATE INDEX forge_access_asked ON forge_access (asked_at);

-- A `member`, `membership`, `organization`, `repository`, `team`,
-- `team_add` or installation event drops every answer asked at or before
-- it for its `scope`: `repo` (`owner/name`), `owner` (a login) or
-- `person` (a GitHub id), in lower case. `at` is the database's clock.
-- Kept as long as an answer is.
CREATE TABLE forge_access_drops (
    scope TEXT COLLATE pg_c_utf8 NOT NULL CHECK (scope IN ('repo', 'owner', 'person')),
    subject TEXT COLLATE pg_c_utf8 NOT NULL,
    at BIGINT NOT NULL,
    PRIMARY KEY (scope, subject)
);
