-- 0108: the hosts a workspace's clone secret (`GIT_TOKEN`) may be sent to,
-- as a JSON array, bound when it's set (`hosted::setup`). NULL for every
-- other secret, and for a `GIT_TOKEN` set before: it's sent nowhere until
-- it's set again.
ALTER TABLE hosted_workspace_secrets ADD COLUMN hosts TEXT;
