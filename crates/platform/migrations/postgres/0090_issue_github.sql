-- 0089: Open on GitHub, per issue (M205, Arugula #62). A native issue a
-- person sends to a linked repo, and the comments that cross between the
-- two once it's there. `forge::export` has the rules. Times are
-- milliseconds since the epoch.
--
-- SQLite's file in Postgres's words: `BIGINT` for its integers and
-- `COLLATE pg_c_utf8` text.

ALTER TABLE drafts ADD COLUMN native_issue TEXT COLLATE pg_c_utf8;
ALTER TABLE drafts ADD COLUMN native_seq BIGINT;
CREATE INDEX drafts_native_issue ON drafts (native_issue) WHERE native_issue IS NOT NULL;

CREATE TABLE issue_github_comments (
    github_id BIGINT PRIMARY KEY,
    issue_id TEXT COLLATE pg_c_utf8 NOT NULL,
    seq BIGINT,
    way TEXT COLLATE pg_c_utf8 NOT NULL CHECK (way IN ('in', 'out')),
    at BIGINT NOT NULL
);
CREATE INDEX issue_github_comments_issue ON issue_github_comments (issue_id, seq);
