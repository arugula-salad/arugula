defmodule PresenceRelay.Application do
  @moduledoc false
  use Application

  @impl true
  def start(_type, _args) do
    PresenceRelay.Apps.load()

    repo =
      if Application.get_env(:presence_relay, :database, true), do: [PresenceRelay.Repo], else: []

    children =
      repo ++
        [
          # Nodes find each other on Fly (DNS_CLUSTER_QUERY); locally, ignored.
          {DNSCluster,
           query: Application.get_env(:presence_relay, :dns_cluster_query) || :ignore},
          {Phoenix.PubSub, name: PresenceRelay.PubSub},
          # Every room's connections, cluster-wide (PresenceRelay.Rooms).
          %{id: :pg, start: {:pg, :start_link, [PresenceRelay.Rooms.scope()]}},
          PresenceRelay.Presence,
          PresenceRelay.Rooms,
          PresenceRelay.Chat.Memory,
          PresenceRelayWeb.Endpoint
        ]

    Supervisor.start_link(children, strategy: :one_for_one, name: PresenceRelay.Supervisor)
  end

  @impl true
  def config_change(changed, _new, removed) do
    PresenceRelayWeb.Endpoint.config_change(changed, removed)
    :ok
  end
end
