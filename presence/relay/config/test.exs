import Config

config :presence_relay, PresenceRelayWeb.Endpoint,
  http: [ip: {127, 0, 0, 1}, port: 4002],
  secret_key_base: "test-only-presence-relay-secret-key-base-0123456789abcdef0123456789abcdef",
  server: false

config :presence_relay,
  apps: %{"acme" => "acme-test-secret", "other" => "other-test-secret"},
  demo: true,
  # Quick sweeps so the stale-frame test doesn't wait 10 s.
  sweep_ms: 50,
  stale_ms: 300

config :presence_relay, PresenceRelay.Repo,
  url:
    System.get_env("TEST_DATABASE_URL") ||
      "postgres://postgres:postgres@127.0.0.1:55432/presence_test#{System.get_env("MIX_TEST_PARTITION")}",
  pool: Ecto.Adapters.SQL.Sandbox,
  pool_size: System.schedulers_online() * 2

config :presence_relay, PresenceRelay.Mailer, adapter: Swoosh.Adapters.Test
config :swoosh, :api_client, false
config :presence_relay, seed_meet: false

config :presence_relay, PresenceRelay.Accounts.Google,
  client_id: "test-client",
  client_secret: "test-secret",
  client: PresenceRelay.FakeGoogle

config :logger, level: :warning
config :phoenix, :plug_init_mode, :runtime
