-- 0029: an environment's driver settings (M220, platform step 7). What a
-- driver needs that isn't secret, as a JSON object of names to one-word
-- values: for chant-fly, the Fly app (`fly_app`), where its chant op is
-- (`chant_dir`, `chant_op`) and the Fly config a roll back deploys with
-- (`fly_config`). The deploy's plan names each, so an owner's approval
-- covers them, and a box checks them again. Secrets are never here.
ALTER TABLE environments ADD COLUMN settings TEXT COLLATE pg_c_utf8 NOT NULL DEFAULT '{}';
