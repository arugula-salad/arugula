-- 0092: a GitHub comment held while a comment of ours with the same text is
-- being sent to the same exported issue (Arugula #585). It may be our copy
-- or someone's who wrote the same words: the send's answer names ours, and
-- the rest are then brought in as GitHub's. `forge::export` has the rules.
-- A row lives only until that send ends. Times are milliseconds since the
-- epoch.
CREATE TABLE issue_github_held (
    github_id INTEGER PRIMARY KEY,
    issue_id TEXT NOT NULL,
    login TEXT NOT NULL,
    body TEXT NOT NULL,
    at INTEGER NOT NULL
);
CREATE INDEX issue_github_held_issue ON issue_github_held (issue_id);
