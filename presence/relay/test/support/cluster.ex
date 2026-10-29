defmodule PresenceRelay.Cluster do
  @moduledoc """
  A second node for cluster tests (#318's P2): this test node goes
  distributed, and an OTP `:peer` starts beside it with the same code and
  config, running the relay. PubSub, Presence and `:pg` join them up as on
  Fly.
  """

  @apps [:logger, :phoenix, :presence_relay]

  def ensure_distributed! do
    unless Node.alive?() do
      {_, 0} = System.cmd("epmd", ["-daemon"])
      name = :"relay_test_#{System.unique_integer([:positive])}@127.0.0.1"
      {:ok, _} = :net_kernel.start(name, %{name_domain: :longnames})
    end

    :ok
  end

  @doc "A peer node running the relay; `{peer_pid, node}`."
  def start_peer! do
    ensure_distributed!()
    name = :"relay_peer_#{System.unique_integer([:positive])}"
    args = Enum.flat_map(:code.get_path(), &[~c"-pa", &1])

    {:ok, peer, node} =
      :peer.start_link(%{name: name, host: ~c"127.0.0.1", longnames: true, args: args})

    for app <- @apps do
      :ok = :erpc.call(node, Application, :put_all_env, [[{app, Application.get_all_env(app)}]])
    end

    # The cluster tests don't touch Postgres: no Repo (and no pool) there.
    :ok = :erpc.call(node, Application, :put_env, [:presence_relay, :database, false])
    {:ok, _} = :erpc.call(node, Application, :ensure_all_started, [:presence_relay])
    {peer, node}
  end

  @doc "Sets `key` in the relay's env on this node and `node`."
  def put_env(node, key, value) do
    Application.put_env(:presence_relay, key, value)
    :ok = :erpc.call(node, Application, :put_env, [:presence_relay, key, value])
  end
end
