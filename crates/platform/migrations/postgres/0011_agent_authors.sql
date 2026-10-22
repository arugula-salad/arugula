-- 0011: messages an agent wrote (M197, platform step 6, #1263). An agent
-- acts through its machine's grant ("Grants, not impersonation"), and what
-- it writes keeps its owner's account as `author`, with the machine in
-- `agent`: shown as "<owner>'s agent on <machine>". NULL for a person's.
-- A revoked grant leaves its machine's messages as they are, still
-- labelled. SQLite's file in Postgres's words.
ALTER TABLE chat_messages ADD COLUMN agent TEXT COLLATE pg_c_utf8;
