-- 0101: native issues by their GitHub origin. See SQLite's file.
CREATE INDEX issues_github ON issues (github_repo_id, github_number) WHERE github_number IS NOT NULL;
