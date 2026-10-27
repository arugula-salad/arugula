-- 0062: how many session users a hosted workspace's box has (Arugula #215),
-- as its daemon says on each pull (`session_users`): the most sessions it
-- holds is the fewer of that and six, and runs, of that and three. NULL
-- for a box that hasn't said, today's template: three.
ALTER TABLE hosted_workspaces ADD COLUMN session_users INTEGER;
