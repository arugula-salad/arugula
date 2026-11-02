-- 0089: Open on GitHub, per issue (M205, Arugula #62). A native issue a
-- person sends to a linked repo, and the comments that cross between the
-- two once it's there. `forge::export` has the rules. Times are
-- milliseconds since the epoch.

-- A draft made for a native issue (`native_issue`, its `iss_` id): the
-- issue itself, sent as a new GitHub issue (`kind = 'issue'`), or one of
-- its comments, sent as a comment on the GitHub issue it went to
-- (`kind = 'comment'`, `native_seq` the comment's number in the issue's
-- thread). When one is sent the issue's GitHub origin (`issues.github_*`,
-- 0009) is set, or the comment is linked below.
ALTER TABLE drafts ADD COLUMN native_issue TEXT;
ALTER TABLE drafts ADD COLUMN native_seq INTEGER;
CREATE INDEX drafts_native_issue ON drafts (native_issue) WHERE native_issue IS NOT NULL;

-- A comment that crossed between an exported issue's thread and its
-- GitHub issue: GitHub's comment id, the native issue, the message's
-- number in its thread (NULL while it's being brought in), and which way
-- it went (`in`: GitHub's, brought into the thread; `out`: a person's,
-- sent to GitHub). One row per GitHub comment, so neither way brings a
-- comment back where it came from.
CREATE TABLE issue_github_comments (
    github_id INTEGER PRIMARY KEY,
    issue_id TEXT NOT NULL,
    seq INTEGER,
    way TEXT NOT NULL CHECK (way IN ('in', 'out')),
    at INTEGER NOT NULL
);
CREATE INDEX issue_github_comments_issue ON issue_github_comments (issue_id, seq);
