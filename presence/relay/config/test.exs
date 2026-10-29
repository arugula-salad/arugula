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

config :logger, level: :warning
config :phoenix, :plug_init_mode, :runtime
