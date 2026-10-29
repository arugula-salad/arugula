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

config :logger, :console, format: "$time $metadata[$level] $message\n"
config :phoenix, :json_library, Jason

import_config "#{config_env()}.exs"
