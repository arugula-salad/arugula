defmodule PresenceRelay.Application do
  @moduledoc false
  use Application
  require Logger

  @impl true
  def start(_type, _args) do
    PresenceRelay.Apps.load()
    PresenceRelay.Limits.configure!()

    # A live Stripe key without STRIPE_LIVE=1 (or half a config) stops the
    # boot here (PresenceRelay.Billing).
    stripe = Application.get_env(:presence_relay, :stripe)
    PresenceRelay.Billing.check_config!(stripe)

    cond do
      stripe == nil ->
        Logger.info("no Stripe config: billing is off (caps are the owners' own)")

      PresenceRelay.Billing.test_mode?() ->
        Logger.info("billing on, in Stripe test mode")

      true ->
        Logger.warning("billing on, with a LIVE Stripe key (STRIPE_LIVE=1)")
    end

    if PresenceRelay.Mailer.ready?() == false,
      do: Logger.warning("no mail sender (RESEND_API_KEY unset): email sign-in is off")

    db? = Application.get_env(:presence_relay, :database, true)
    repo = if db?, do: [PresenceRelay.Repo], else: []

    # The system account and app `meet`, once the Repo is up.
    seed =
      if db? and Application.get_env(:presence_relay, :seed_meet, true),
        do: [Supervisor.child_spec({Task, &PresenceRelay.Apps.ensure_meet!/0}, id: :seed_meet)],
        else: []

    # Metrics first, so it hears everything from the start (#318's P12).
    children =
      [PresenceRelay.Metrics] ++
        repo ++
        [
          {Finch, name: PresenceRelay.Finch},
          # Abuse limits' counts (PresenceRelay.Limits).
          PresenceRelay.Limits,
          # Sign-in providers' keys (JWKS), cached per URL.
          PresenceRelay.Issuers.Keys,
          # Nodes find each other on Fly (DNS_CLUSTER_QUERY); locally, ignored.
          {DNSCluster,
           query: Application.get_env(:presence_relay, :dns_cluster_query) || :ignore},
          {Phoenix.PubSub, name: PresenceRelay.PubSub},
          # Every room's connections, cluster-wide (PresenceRelay.Rooms).
          %{id: :pg, start: {:pg, :start_link, [PresenceRelay.Rooms.scope()]}},
          PresenceRelay.Presence,
          PresenceRelay.Rooms,
          # Usage (monthly active users, call minutes) and caps, batched.
          PresenceRelay.Usage,
          # Team apps' usage to Stripe's meters, every 15 minutes (billing on).
          PresenceRelay.Billing.Reporter,
          PresenceRelay.Chat.Memory,
          PresenceRelayWeb.Endpoint,
          # Mails when deep health fails (PresenceRelay.Health).
          PresenceRelay.Watchdog
        ] ++ metrics() ++ seed

    Supervisor.start_link(children, strategy: :one_for_one, name: PresenceRelay.Supervisor)
  end

  # /metrics on its own port, where config says (PresenceRelayWeb.MetricsPlug).
  defp metrics do
    case Application.get_env(:presence_relay, :metrics_port) do
      port when is_integer(port) ->
        ip = Application.get_env(:presence_relay, :metrics_ip, {127, 0, 0, 1})
        opts = [plug: PresenceRelayWeb.MetricsPlug, port: port, ip: ip, startup_log: false]
        [Supervisor.child_spec({Bandit, opts}, id: :metrics)]

      _ ->
        []
    end
  end

  @impl true
  def config_change(changed, _new, removed) do
    PresenceRelayWeb.Endpoint.config_change(changed, removed)
    :ok
  end
end
