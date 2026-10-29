import Config

config :presence_relay, PresenceRelayWeb.Endpoint,
  adapter: Bandit.PhoenixAdapter,
  url: [host: "localhost"],
  render_errors: [formats: [json: PresenceRelayWeb.ErrorJSON], layout: false],
  pubsub_server: PresenceRelay.PubSub

config :presence_relay,
  ecto_repos: [PresenceRelay.Repo],
  # Whether there's a database (Postgres: accounts, apps, messages, usage).
  # A relay without one still relays (the demo, before #318's P1).
  database: true,
  # app id => HS256 secret; runtime.exs fills it from PRESENCE_APPS.
  apps: %{},
  demo: false

# Mail (magic links): Resend in prod when RESEND_API_KEY is set
# (runtime.exs); the log in dev, Swoosh's test adapter in tests.
config :swoosh, api_client: Swoosh.ApiClient.Finch, finch_name: PresenceRelay.Finch

config :presence_relay, PresenceRelay.Mailer,
  adapter: Swoosh.Adapters.Logger,
  level: :info,
  log_full_email: true

config :presence_relay, mail_from: {"arugula.stream", "hello@arugula.stream"}

# Signed session cookies (the dashboard, meet.arugula.stream).
config :presence_relay, :session,
  store: :cookie,
  key: "_arugula_stream",
  signing_salt: "arugula-stream-session",
  same_site: "Lax",
  max_age: 30 * 24 * 3600

# meet.arugula.stream's origins (app `meet`); MEET_ORIGINS overrides.
config :presence_relay, meet_origins: ["https://meet.arugula.stream"], seed_meet: true

# Abuse limits (#318's P12; PresenceRelay.Limits): {hits, seconds} per
# client IP, or per address for email_address; nil turns one off.
# RATE_LIMITS (runtime.exs) changes them. Counted on each node.
config :presence_relay, :rate_limits, %{
  socket_connect: {120, 60},
  email_ip: {10, 3600},
  email_address: {5, 3600},
  meet_room: {20, 3600},
  api_write: {120, 60},
  demo_token: {120, 3600}
}

# An app's most connections at once, across the cluster (PresenceRelay.
# Rooms); app_connections (app id => n, or nil for none) overrides it per
# app. APP_MAX_CONNECTIONS and APP_CONNECTIONS (runtime.exs) set them.
config :presence_relay, max_app_connections: 2_000, app_connections: %{}

# Whether Fly-Client-IP is the client's IP (PresenceRelayWeb.ClientIP):
# only behind Fly's proxy, which sets it (runtime.exs, on Fly).
config :presence_relay, trust_fly_client_ip: false

# /metrics on its own port (PresenceRelay.Metrics; METRICS_PORT), off
# unless set; the watchdog's mail when deep health fails
# (PresenceRelay.Watchdog; ALERT_EMAIL).
config :presence_relay,
  metrics_port: nil,
  alert_to: "hello@arugula.stream",
  watchdog_ms: 60_000,
  alert_after: 3

config :logger, :console, format: "$time $metadata[$level] $message\n"
config :phoenix, :json_library, Jason

# Phoenix's log lines (requests, socket connects, joins) blank these
# parameters' values: credentials and addresses never reach the logs.
config :phoenix,
       :filter_parameters,
       ~w(password secret token auth key email code state gid api_key)

import_config "#{config_env()}.exs"
