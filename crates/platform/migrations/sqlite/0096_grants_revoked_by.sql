-- 0096: who revoked a grant (Arugula #683). Until now only its giver (or,
-- for cloud.drive, the account it's over) could, so `granted_by` said who;
-- a project's owner may now revoke any grant over the project, so the
-- revoke writes the account that did it. NULL on a row revoked before
-- this: its giver did.
ALTER TABLE grants ADD COLUMN revoked_by TEXT;
