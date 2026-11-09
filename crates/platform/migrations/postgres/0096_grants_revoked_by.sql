-- 0096: who revoked a grant (Arugula #683). See SQLite's file.
ALTER TABLE grants ADD COLUMN revoked_by TEXT COLLATE pg_c_utf8;
