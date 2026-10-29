defmodule PresenceRelay.Release do
  @moduledoc "Release tasks: `bin/presence_relay eval PresenceRelay.Release.migrate()`."

  def migrate do
    Application.load(:presence_relay)

    # A relay without DATABASE_URL (database: false) has nothing to migrate.
    if Application.get_env(:presence_relay, :database, true) do
      for repo <- Application.fetch_env!(:presence_relay, :ecto_repos) do
        {:ok, _, _} = Ecto.Migrator.with_repo(repo, &Ecto.Migrator.run(&1, :up, all: true))
      end
    else
      IO.puts("no database configured: nothing to migrate")
    end
  end
end
