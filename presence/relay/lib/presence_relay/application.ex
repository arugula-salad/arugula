defmodule PresenceRelay.Application do
  @moduledoc false
  use Application
  require Logger

  @impl true
  def start(_type, _args) do
    PresenceRelay.Apps.load()

    if PresenceRelay.Mailer.ready?() == false,
      do: Logger.warning("no mail sender (RESEND_API_KEY unset): email sign-in is off")

    db? = Application.get_env(:presence_relay, :database, true)
    repo = if db?, do: [PresenceRelay.Repo], else: []

    # The system account and app `meet`, once the Repo is up.
    seed =
      if db? and Application.get_env(:presence_relay, :seed_meet, true),
        do: [Supervisor.child_spec({Task, &PresenceRelay.Apps.ensure_meet!/0}, id: :seed_meet)],
        else: []

    children =
      repo ++
        [
          {Finch, name: PresenceRelay.Finch},
          {Phoenix.PubSub, name: PresenceRelay.PubSub},
          PresenceRelay.Presence,
          PresenceRelay.Rooms,
          PresenceRelayWeb.Endpoint
        ] ++ seed

    Supervisor.start_link(children, strategy: :one_for_one, name: PresenceRelay.Supervisor)
  end

  @impl true
  def config_change(changed, _new, removed) do
    PresenceRelayWeb.Endpoint.config_change(changed, removed)
    :ok
  end
end
