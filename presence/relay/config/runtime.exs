import Config

# PRESENCE_APPS="id:secret,id2:secret2": each host app's id and the HS256
# secret its server signs room tokens with. DEMO=1 adds app "demo" (a
# random secret at boot) and POST /api/demo/token.
# DNS_CLUSTER_QUERY (e.g. "arugula-stream.internal" on Fly, set by
# rel/env.sh.eex) lets the nodes find each other; unset, a lone node.
config :presence_relay, dns_cluster_query: System.get_env("DNS_CLUSTER_QUERY")

# Sign in with Google: on when both are set. The callback is
# /auth/google/callback on whichever host the browser is on.
if config_env() != :test do
  config :presence_relay, PresenceRelay.Accounts.Google,
    client_id: System.get_env("GOOGLE_CLIENT_ID"),
    client_secret: System.get_env("GOOGLE_CLIENT_SECRET")

  if origins = System.get_env("MEET_ORIGINS"),
    do: config(:presence_relay, meet_origins: String.split(origins, ",", trim: true))

  if from = System.get_env("MAIL_FROM") do
    from =
      case Regex.run(~r/\A\s*(.*?)\s*<([^>]+)>\s*\z/, from) do
        [_, name, address] -> {name, address}
        _ -> {"arugula.stream", String.trim(from)}
      end

    config :presence_relay, mail_from: from
  end
end

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

  # Calls (#318's P7) on LiveKit: LIVEKIT_URL (the wss:// URL pages
  # connect to), LIVEKIT_API_KEY and LIVEKIT_API_SECRET (what the relay
  # signs call tokens with). All three, or none: calls off, and the relay
  # says so to every page.
  livekit =
    case Enum.map(~w(LIVEKIT_URL LIVEKIT_API_KEY LIVEKIT_API_SECRET), &(System.get_env(&1) || "")) do
      ["", "", ""] ->
        nil

      [url, key, secret] when url != "" and key != "" and secret != "" ->
        %{url: url, key: key, secret: secret}

      _ ->
        raise "LIVEKIT_URL, LIVEKIT_API_KEY and LIVEKIT_API_SECRET go together: set all three, or none"
    end

  # Billing (#318's P11) on Stripe: STRIPE_SECRET_KEY (a test key,
  # sk_test_…; a live one refuses to boot unless STRIPE_LIVE=1, which Jake
  # sets when he goes live), STRIPE_WEBHOOK_SECRET (whsec_…, from the
  # webhook endpoint or `stripe listen`) and the three Team prices'
  # ids, which scripts/stripe_setup.exs makes and prints. No key: billing
  # off. STRIPE_API_BASE points it elsewhere (stripe-mock);
  # STRIPE_API_VERSION pins another API version.
  stripe =
    case System.get_env("STRIPE_SECRET_KEY") do
      key when key in [nil, ""] ->
        nil

      key ->
        %{
          secret_key: key,
          webhook_secret: System.get_env("STRIPE_WEBHOOK_SECRET"),
          prices: %{
            base: System.get_env("STRIPE_PRICE_TEAM_BASE"),
            mau: System.get_env("STRIPE_PRICE_TEAM_MAU"),
            minutes: System.get_env("STRIPE_PRICE_TEAM_MINUTES")
          },
          api_base: System.get_env("STRIPE_API_BASE") || "https://api.stripe.com",
          # Stripe as merchant of record (Managed Payments): on unless 0.
          managed_payments: System.get_env("STRIPE_MANAGED_PAYMENTS") not in ~w(0 false),
          live: System.get_env("STRIPE_LIVE") in ~w(1 true)
        }
        |> then(fn c ->
          case System.get_env("STRIPE_API_VERSION") do
            v when v in [nil, ""] -> c
            v -> Map.put(c, :api_version, v)
          end
        end)
    end

  # How often each node tells Stripe's meters (default 15 minutes);
  # shorter for the test-mode check.
  if ms = System.get_env("BILLING_REPORT_MS"),
    do: config(:presence_relay, billing_report_ms: String.to_integer(ms))

  config :presence_relay,
    stripe: stripe,
    livekit: livekit,
    apps: apps,
    demo: System.get_env("DEMO") in ~w(1 true),
    demo_secret: System.get_env("DEMO_SECRET")
end

if config_env() == :prod do
  # Mail by Resend when RESEND_API_KEY is set; otherwise none (email
  # sign-in says it isn't set up; guests and Google still work).
  case System.get_env("RESEND_API_KEY") do
    key when is_binary(key) and key != "" ->
      config :presence_relay, PresenceRelay.Mailer, adapter: Swoosh.Adapters.Resend, api_key: key

    _ ->
      config :presence_relay, PresenceRelay.Mailer, adapter: nil
  end

  # Postgres when DATABASE_URL is set (fly mpg attach sets it); a relay
  # without one still relays.
  case System.get_env("DATABASE_URL") do
    nil ->
      config :presence_relay, database: false

    url ->
      config :presence_relay, PresenceRelay.Repo,
        url: url,
        pool_size: String.to_integer(System.get_env("POOL_SIZE") || "10"),
        # Fly's Managed Postgres pools through PgBouncer in transaction
        # mode: no named prepared statements.
        prepare: :unnamed,
        socket_options: if(System.get_env("ECTO_IPV6") in ~w(true 1), do: [:inet6], else: [])
  end

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
