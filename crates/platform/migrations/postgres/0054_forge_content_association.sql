-- 0054: GitHub's `author_association` for each mirrored body, comment,
-- review and review comment (M256, thread = session, Arugula #129): what the
-- author is to the repo (`OWNER`, `MEMBER`, `COLLABORATOR`, `CONTRIBUTOR`,
-- `NONE` …), as GitHub said it with the text. A thread session's prompt
-- fences GitHub's text as quoted data saying whose it is and this. NULL for
-- rows mirrored before it, until GitHub sends the text again.
ALTER TABLE forge_content ADD COLUMN author_association TEXT;
