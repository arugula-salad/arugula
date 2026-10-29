-- Made-up data for trying the backup drill locally (README.md, "Backups"):
-- run against a migrated dev database (mix ecto.setup), then point
-- backup-drill.sh's SOURCE_URL at it. Every row is fake, and every id
-- starts "drill", so it's plain what came from here. Running it again
-- adds nothing new.
BEGIN;

INSERT INTO accounts (id, email, name, kind, inserted_at, updated_at)
SELECT 'acc_drill' || i, 'drill' || i || '@example.test', 'Drill ' || i, 'user', now(), now()
FROM generate_series(1, 200) i
ON CONFLICT DO NOTHING;

INSERT INTO identities (account_id, provider, subject, inserted_at, updated_at)
SELECT 'acc_drill' || i, 'google', 'drill-google-' || i, now(), now()
FROM generate_series(1, 100) i
ON CONFLICT DO NOTHING;

INSERT INTO apps (id, owner_id, name, secret_enc, guests, mau_cap, inserted_at, updated_at)
SELECT 'app_drill' || i, 'acc_drill' || (1 + i % 200), 'Drill app ' || i,
       decode(md5('drill-secret-' || i), 'hex'), i % 2 = 0, CASE WHEN i % 5 = 0 THEN 100 END, now(), now()
FROM generate_series(1, 50) i
ON CONFLICT DO NOTHING;

INSERT INTO app_keys (app_id, key, mode, inserted_at)
SELECT 'app_drill' || i, 'pk_test_drill' || lpad(i::text, 12, '0'), 'test', now()
FROM generate_series(1, 50) i
ON CONFLICT DO NOTHING;

INSERT INTO app_origins (app_id, origin, inserted_at)
SELECT 'app_drill' || i, 'https://drill' || i || '.example.test', now()
FROM generate_series(1, 50) i
ON CONFLICT DO NOTHING;

INSERT INTO login_tokens (token_hash, email, expires_at, inserted_at)
SELECT decode(md5('drill-token-' || i), 'hex'), 'drill' || i || '@example.test', now() + interval '15 minutes', now()
FROM generate_series(1, 30) i
ON CONFLICT DO NOTHING;

INSERT INTO meet_rooms (slug, name, creator_id, created_at)
SELECT 'drill-' || lpad(i::text, 4, '0'), 'Drill room ' || i, CASE WHEN i % 3 = 0 THEN NULL ELSE 'acc_drill' || i END, now()
FROM generate_series(1, 120) i
ON CONFLICT DO NOTHING;

INSERT INTO meet_recent (account_id, slug, joined_at)
SELECT 'acc_drill' || i, 'drill-' || lpad((1 + i % 120)::text, 4, '0'), now()
FROM generate_series(1, 150) i
ON CONFLICT DO NOTHING;

INSERT INTO messages (app, room, sub, name, text, inserted_at)
SELECT 'app_drill' || (1 + i % 50), 'room-' || (i % 20), 'u' || (i % 300), 'Drill ' || (i % 300),
       'drill message ' || i || E'\nwith a second line, an emoji 🎉 and a ''quote''', now() - (i || ' seconds')::interval
FROM generate_series(1, 5000) i
WHERE NOT EXISTS (SELECT 1 FROM messages WHERE app LIKE 'app_drill%');

-- Usage that agrees with itself: each member's days, and each day's
-- active users counted from them (PresenceRelay.Usage).
INSERT INTO usage_members (app_id, month, member_key, days)
SELECT 'app_drill' || (1 + i % 50), date_trunc('month', now())::date, 'drill-member-' || i,
       (1 << (i % 28)) | (1 << ((i * 7) % 28))
FROM generate_series(1, 2000) i
ON CONFLICT DO NOTHING;

INSERT INTO usage_days (app_id, day, active_users, call_seconds)
SELECT m.app_id, (m.month + d)::date, count(*), 60 * count(*)
FROM usage_members m, generate_series(0, 27) d
WHERE m.member_key LIKE 'drill-member-%' AND (m.days & (1 << d)) <> 0
GROUP BY m.app_id, m.month, d
ON CONFLICT DO NOTHING;

COMMIT;
