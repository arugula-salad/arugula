import Config

config :presence_relay, PresenceRelayWeb.Endpoint,
  http: [ip: {127, 0, 0, 1}, port: 4000],
  check_origin: false,
  debug_errors: true,
  secret_key_base: "dev-only-presence-relay-secret-key-base-0123456789abcdef0123456789abcdef"

config :phoenix, :stacktrace_depth, 20
config :phoenix, :plug_init_mode, :runtime

# A local Postgres (docker run -p 127.0.0.1:55432:5432 postgres:17), or
# DATABASE_URL.
config :presence_relay, PresenceRelay.Repo,
  url:
    System.get_env("DATABASE_URL") || "postgres://postgres:postgres@127.0.0.1:55432/presence_dev",
  show_sensitive_data_on_connection_error: true,
  pool_size: 10

# meet.arugula.stream in dev, on this relay's port.
config :presence_relay, meet_origins: ["https://meet.arugula.stream", "http://localhost:4203"]
