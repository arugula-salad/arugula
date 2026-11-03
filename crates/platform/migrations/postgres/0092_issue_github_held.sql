-- 0092: a GitHub comment held while a comment of ours with the same text is
-- being sent to the same exported issue (Arugula #585). It may be our copy
-- or someone's who wrote the same words: the send's answer names ours, and
-- the rest are then brought in as GitHub's. `forge::export` has the rules.
-- A row lives only until that send ends. Times are milliseconds since the
-- epoch.
--
-- SQLite's file in Postgres's words: `BIGINT` for its integers and
-- `COLLATE pg_c_utf8` text.

CREATE TABLE issue_github_held (
    github_id BIGINT PRIMARY KEY,
    issue_id TEXT COLLATE pg_c_utf8 NOT NULL,
    login TEXT COLLATE pg_c_utf8 NOT NULL,
    body TEXT COLLATE pg_c_utf8 NOT NULL,
    at BIGINT NOT NULL
);
CREATE INDEX issue_github_held_issue ON issue_github_held (issue_id);
