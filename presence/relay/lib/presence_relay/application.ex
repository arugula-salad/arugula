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
          {Phoenix.PubSub, name: PresenceRelay.PubSub},
          PresenceRelay.Presence,
          PresenceRelay.Rooms,
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
