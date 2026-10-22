-- 0012: a project's issue home (M195, platform step 6, #1263): where its
-- "new issue" goes, `native` (the platform's own issues, M194) or `github`
-- (its linked repo). NULL is the default: `github` while the project has a
-- linked repo, `native` when it has none (`crate::issues::home`), so a
-- project linked to GitHub keeps GitHub until someone switches it.
ALTER TABLE projects ADD COLUMN issue_home TEXT CHECK (issue_home IN ('native', 'github'));
