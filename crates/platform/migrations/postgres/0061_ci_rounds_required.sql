-- 0061: CI rounds that count (Arugula #210). `app` is the GitHub App
-- whose check run an item is (`github-actions` for an Actions job, whose
-- log a round sends: `sessions::feedback`); NULL for a commit status, a
-- review or a comment. `not_required` marks a CI batch dropped because
-- none of its failed checks is one the base branch requires: it spent no
-- round, and a check that fails on its commit later opens it again.
ALTER TABLE session_feedback_items ADD COLUMN app TEXT COLLATE pg_c_utf8;
ALTER TABLE session_feedback ADD COLUMN not_required BOOLEAN NOT NULL DEFAULT FALSE;
