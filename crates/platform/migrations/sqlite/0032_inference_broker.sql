-- 0032: the inference broker (M231, platform step 8, Arugula #29). A
-- project's members, and registered clients (studio's lobby) for the
-- accounts they name, reach Claude through the platform, which sets the
-- key on the way out: the project's own (`project_inference_keys`), an
-- account's own (its personal project's), or the platform's under a
-- monthly cap ("The Claude credential broker and its metering move to
-- the platform", amending "Hosted inference: strangers bring their own
-- key", aru-096). Beside step 5's `hosted_turns`, which counts agent
-- turns on hosted boxes, this prices each call the broker made.
--
-- Times are milliseconds since the epoch.

-- A call the broker made to the provider (a Messages API request),
-- written when its answer ended, cut short or not.
-- - `project_id`: the project it was made for.
-- - `account`: whose call it was (the member, or the account the client
--   named); `client`: the registered client that made it (`studio`), NULL
--   for a member's own. NULL once that account is deleted.
-- - `pays`: whose key: `project`, `account` (`payer`'s own key: the
--   account itself, its sponsor or a visitor, as the client said), or
--   `platform` (counted against `account`'s monthly cap, per client).
-- - `model`, `status` (the provider's), the usage the provider reported,
--   and `cost_micros`: its list price in millionths of a US dollar
--   (`hosted::broker::prices`), `priced_as` the price row it used (the
--   dearest for a model the table doesn't name).
-- Kept 90 days.
CREATE TABLE inference_calls (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL,
    account TEXT,
    client TEXT,
    provider TEXT NOT NULL CHECK (provider IN ('anthropic', 'openai')),
    pays TEXT NOT NULL CHECK (pays IN ('project', 'account', 'platform')),
    payer TEXT,
    model TEXT,
    status INTEGER NOT NULL,
    input_tokens INTEGER NOT NULL DEFAULT 0,
    output_tokens INTEGER NOT NULL DEFAULT 0,
    cache_write_tokens INTEGER NOT NULL DEFAULT 0,
    cache_write_1h_tokens INTEGER NOT NULL DEFAULT 0,
    cache_read_tokens INTEGER NOT NULL DEFAULT 0,
    cost_micros INTEGER NOT NULL DEFAULT 0,
    priced_as TEXT,
    started_at INTEGER NOT NULL,
    ended_at INTEGER NOT NULL
);
CREATE INDEX inference_calls_project ON inference_calls (project_id, started_at);
CREATE INDEX inference_calls_account ON inference_calls (account, started_at);
CREATE INDEX inference_calls_payer ON inference_calls (payer);
