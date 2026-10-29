import Config

config :presence_relay, PresenceRelayWeb.Endpoint,
  http: [ip: {127, 0, 0, 1}, port: 4002],
  secret_key_base: "test-only-presence-relay-secret-key-base-0123456789abcdef0123456789abcdef",
  server: false

config :presence_relay,
  apps: %{"acme" => "acme-test-secret", "other" => "other-test-secret"},
  demo: true,
  # The demo's index without building the demo.
  index_html: Path.expand("../test/support/index.html", __DIR__),
  meet_index_html: Path.expand("../test/support/meet.html", __DIR__),
  dashboard_index_html: Path.expand("../test/support/dashboard.html", __DIR__),
  # Quick sweeps so the stale-frame test doesn't wait 10 s.
  sweep_ms: 50,
  stale_ms: 300,
  # Calls on, against no real LiveKit (tests read the tokens); rings that
  # give up quickly.
  livekit: %{url: "wss://livekit.test", key: "test-key", secret: "test-livekit-secret"},
  ring_ms: 300,
  # Usage is written when a test says (PresenceRelay.Usage.flush/0).
  usage_flush_ms: :manual,
  # Billing off unless a test turns it on (against a fake Stripe), and
  # meter events sent when a test says (PresenceRelay.Billing.Reporter.run/1).
  stripe: nil,
  billing_report_ms: :manual

# Abuse limits out of the way (the limits' own tests set theirs), and no
# watchdog.
config :presence_relay,
  rate_limits: %{
    socket_connect: {1_000_000, 60},
    email_ip: {1_000_000, 60},
    email_address: {1_000_000, 60},
    meet_room: {1_000_000, 60},
    api_write: {1_000_000, 60},
    demo_token: {1_000_000, 60}
  },
  watchdog_ms: :manual

config :presence_relay, PresenceRelay.Repo,
  url:
    System.get_env("TEST_DATABASE_URL") ||
      "postgres://postgres:postgres@127.0.0.1:55432/presence_test#{System.get_env("MIX_TEST_PARTITION")}",
  pool: Ecto.Adapters.SQL.Sandbox,
  pool_size: System.schedulers_online() * 2

config :presence_relay, PresenceRelay.Mailer, adapter: Swoosh.Adapters.Test
config :swoosh, :api_client, false
config :presence_relay, seed_meet: false

# Sign-in providers: the tests' own issuers are on 127.0.0.1 over http,
# a refetch for an unknown key id is allowed sooner than in prod, and a
# slow issuer gives up sooner.
config :presence_relay, issuer_allow_local: true, jwks_refetch_ms: 300, jwks_timeout_ms: 500

config :presence_relay, PresenceRelay.Accounts.Google,
  client_id: "test-client",
  client_secret: "test-secret",
  client: PresenceRelay.FakeGoogle

config :logger, level: :warning
config :phoenix, :plug_init_mode, :runtime
