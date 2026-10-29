defmodule PresenceRelay.DataCase do
  @moduledoc "Tests that touch Postgres: each in a sandboxed transaction."
  use ExUnit.CaseTemplate

  using do
    quote do
      alias PresenceRelay.Repo
      import Ecto.Query
      import PresenceRelay.DataCase
    end
  end

  setup tags do
    pid = Ecto.Adapters.SQL.Sandbox.start_owner!(PresenceRelay.Repo, shared: not tags[:async])
    on_exit(fn -> Ecto.Adapters.SQL.Sandbox.stop_owner(pid) end)
    :ok
  end
end
