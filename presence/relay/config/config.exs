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

config :logger, :console, format: "$time $metadata[$level] $message\n"
config :phoenix, :json_library, Jason

import_config "#{config_env()}.exs"
