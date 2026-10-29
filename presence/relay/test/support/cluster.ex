defmodule PresenceRelay.Cluster do
  @moduledoc """
  A second node for cluster tests (#318's P2): this test node goes
  distributed, and an OTP `:peer` starts beside it with the same code and
  config, running the relay. PubSub, Presence and `:pg` join them up as on
  Fly.
  """

  @apps [:logger, :phoenix, :swoosh, :presence_relay]

  def ensure_distributed! do
    unless Node.alive?() do
      {_, 0} = System.cmd("epmd", ["-daemon"])
      name = :"relay_test_#{System.unique_integer([:positive])}@127.0.0.1"
      {:ok, _} = :net_kernel.start(name, %{name_domain: :longnames})
    end

    :ok
  end

  @doc """
  A peer node running the relay; `{peer_pid, node}`. `database: true`
  gives it a Repo on the test database, outside the sandbox (the usage
  load test commits, and cleans up after itself).
  """
  def start_peer!(opts \\ []) do
    ensure_distributed!()
    name = :"relay_peer_#{System.unique_integer([:positive])}"
    args = Enum.flat_map(:code.get_path(), &[~c"-pa", &1])

    {:ok, peer, node} =
      :peer.start_link(%{name: name, host: ~c"127.0.0.1", longnames: true, args: args})

    # Loaded first, so an app's own defaults (swoosh's hackney client)
    # don't replace this node's config when it starts.
    for app <- @apps do
      _ = :erpc.call(node, Application, :load, [app])
      :ok = :erpc.call(node, Application, :put_all_env, [[{app, Application.get_all_env(app)}]])
    end

    # A plain pool there (no sandbox), and a small one: the test node's is
    # already most of what a local Postgres allows.
    repo =
      Application.get_env(:presence_relay, PresenceRelay.Repo)
      |> Keyword.merge(pool: DBConnection.ConnectionPool, pool_size: 4)

    :ok = :erpc.call(node, Application, :put_env, [:presence_relay, PresenceRelay.Repo, repo])

    # The cluster tests don't touch Postgres (no Repo, and no pool, there)
    # unless they ask.
    :ok =
      :erpc.call(node, Application, :put_env, [
        :presence_relay,
        :database,
        Keyword.get(opts, :database, false)
      ])

    {:ok, _} = :erpc.call(node, Application, :ensure_all_started, [:presence_relay])
    {peer, node}
  end

  @doc "Sets `key` in the relay's env on this node and `node`."
  def put_env(node, key, value) do
    Application.put_env(:presence_relay, key, value)
    :ok = :erpc.call(node, Application, :put_env, [:presence_relay, key, value])
  end
end
