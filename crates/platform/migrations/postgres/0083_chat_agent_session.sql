-- 0083: the session that wrote an agent's chat message (Arugula #429): its
-- name as the machine called it then, shown beside "<owner>'s agent on
-- <machine>". Display only. Additive: the build before this one ignores
-- the column.
ALTER TABLE chat_messages ADD COLUMN agent_session TEXT COLLATE pg_c_utf8;
