import Config

# PRESENCE_APPS="id:secret,id2:secret2": each host app's id and the HS256
# secret its server signs room tokens with. DEMO=1 adds app "demo" (a
# random secret at boot) and POST /api/demo/token.
if config_env() != :test do
  apps =
    (System.get_env("PRESENCE_APPS") || "")
    |> String.split(",", trim: true)
    |> Map.new(fn pair ->
      case String.split(pair, ":", parts: 2) do
        [id, secret] when id != "" and secret != "" -> {id, secret}
        _ -> raise "PRESENCE_APPS: #{inspect(pair)} isn't id:secret"
      end
    end)

  config :presence_relay, apps: apps, demo: System.get_env("DEMO") in ~w(1 true)
end

if config_env() == :prod do
  secret_key_base =
    System.get_env("SECRET_KEY_BASE") ||
      raise "SECRET_KEY_BASE is missing: mix phx.gen.secret makes one"

  host = System.get_env("PHX_HOST") || "arugula.video"
  port = String.to_integer(System.get_env("PORT") || "4000")

  config :presence_relay, PresenceRelayWeb.Endpoint,
    url: [host: host, port: 443, scheme: "https"],
    http: [ip: {0, 0, 0, 0, 0, 0, 0, 0}, port: port],
    # Host apps embed the client on their own origins; the room token is
    # what admits a socket.
    check_origin: false,
    secret_key_base: secret_key_base,
    server: true
end

# Dev: PORT moves it; BIND listens on another address (a tailnet one, for
# a browser on another machine) instead of loopback.
if config_env() == :dev and (System.get_env("PORT") || System.get_env("BIND")) do
  {:ok, ip} = :inet.parse_address(String.to_charlist(System.get_env("BIND") || "127.0.0.1"))

  config :presence_relay, PresenceRelayWeb.Endpoint,
    http: [ip: ip, port: String.to_integer(System.get_env("PORT") || "4000")]
end
