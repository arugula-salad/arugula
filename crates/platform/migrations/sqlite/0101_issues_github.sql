-- 0101: native issues by their GitHub origin. The forge mirror looks one
-- up each time it first writes a GitHub issue (an export's copy joins its
-- native issue's work item), and an exported issue's comments find their
-- thread by it. Only exported issues have one.
CREATE INDEX issues_github ON issues (github_repo_id, github_number) WHERE github_number IS NOT NULL;
