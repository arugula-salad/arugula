defmodule PresenceRelay.Release do
  @moduledoc "Release tasks: `bin/presence_relay eval PresenceRelay.Release.migrate()`."

  def migrate do
    Application.load(:presence_relay)

    for repo <- Application.fetch_env!(:presence_relay, :ecto_repos) do
      {:ok, _, _} = Ecto.Migrator.with_repo(repo, &Ecto.Migrator.run(&1, :up, all: true))
    end
  end
end
