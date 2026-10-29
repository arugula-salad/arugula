import Config

config :logger, level: :info

# One JSON object a line, scrubbed of tokens, addresses and secrets
# (PresenceRelay.LogFormatter, #318's P12).
config :logger, :default_handler, formatter: {PresenceRelay.LogFormatter, %{}}
