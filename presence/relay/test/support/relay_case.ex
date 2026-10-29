defmodule PresenceRelay.RelayCase do
  @moduledoc """
  Room channel tests: tokens for the test apps, and clients that each
  join from a process of their own (so each has its own pushes), passing
  what they're pushed back to the test as `{:pushed, name, event, payload}`.
  """
  use ExUnit.CaseTemplate

  using do
    quote do
      import Phoenix.ChannelTest
      import PresenceRelay.RelayCase
      @endpoint PresenceRelayWeb.Endpoint
    end
  end

  alias PresenceRelay.Token

  # Joins read a room's history (PresenceRelay.Chat): the channel processes
  # share this test's sandboxed connection.
  setup tags do
    pid = Ecto.Adapters.SQL.Sandbox.start_owner!(PresenceRelay.Repo, shared: not tags[:async])
    on_exit(fn -> Ecto.Adapters.SQL.Sandbox.stop_owner(pid) end)
    :ok
  end

  def token(claims \\ %{}, app \\ "acme") do
    base = %{
      "room" => "lobby",
      "sub" => "u" <> Integer.to_string(System.unique_integer([:positive])),
      "name" => "Someone",
      "exp" => System.system_time(:second) + 3600
    }

    Token.sign(app, Map.merge(base, claims))
  end

  def room, do: "r" <> Integer.to_string(System.unique_integer([:positive]))

  @doc "A client named `name`, joined to `room` as `sub`: `{:ok, pid}` or the join's error."
  def client(name, room, sub, opts \\ []), do: client_on(node(), name, room, sub, opts)

  @doc "`client/4` on `node` (a cluster test's peer): its channel runs there."
  def client_on(node, name, room, sub, opts \\ []) do
    tok =
      token(Map.merge(%{"room" => room, "sub" => sub, "name" => to_string(name)}, Map.new(opts)))

    client_with(node, name, room, %{"token" => tok})
  end

  @doc """
  A client on `node` whose socket connects with `params` (a token, or a
  publishable key's) and `connect_info` (a key's `origin`), joined to `room`.
  """
  def client_with(node, name, room, params, connect_info \\ %{}) do
    test = self()

    {pid, ref} =
      Node.spawn_monitor(node, fn ->
        # On a cluster test's peer there's no ExUnit run: register this
        # process with ExUnit's on-exit handler there, which is all
        # Phoenix.ChannelTest asks of a "test process".
        if node != node(test) do
          {:ok, _} = Application.ensure_all_started(:ex_unit)
          ExUnit.OnExitHandler.register(self())
        end

        {:ok, socket} =
          Phoenix.ChannelTest.__connect__(
            PresenceRelayWeb.Endpoint,
            PresenceRelayWeb.Socket,
            params,
            connect_info: connect_info,
            test_process: if(node == node(test), do: test, else: self())
          )

        case Phoenix.ChannelTest.join(socket, PresenceRelayWeb.RoomChannel, "room:" <> room, %{}) do
          {:ok, reply, socket} ->
            send(test, {:joined, name, reply})
            loop(test, name, socket)

          {:error, reply} ->
            send(test, {:refused, name, reply})
        end
      end)

    receive do
      {:joined, ^name, reply} ->
        Process.demonitor(ref, [:flush])
        {:ok, pid, reply}

      {:refused, ^name, reply} ->
        Process.demonitor(ref, [:flush])
        {:error, reply}

      {:DOWN, ^ref, _, _, why} ->
        raise "#{inspect(name)} didn't join: #{inspect(why)}"
    after
      2000 -> raise "#{inspect(name)} didn't join"
    end
  end

  defp loop(test, name, socket) do
    receive do
      {:push, event, payload} ->
        Phoenix.ChannelTest.push(socket, event, payload)
        loop(test, name, socket)

      :leave ->
        Process.unlink(socket.channel_pid)
        ref = Process.monitor(socket.channel_pid)
        Phoenix.ChannelTest.close(socket)
        receive do: ({:DOWN, ^ref, _, _, _} -> :ok)
        send(test, {:left, name})

      %Phoenix.Socket.Message{event: event, payload: payload} ->
        send(test, {:pushed, name, event, payload})
        loop(test, name, socket)

      %Phoenix.Socket.Reply{status: status, payload: payload} ->
        send(test, {:replied, name, status, payload})
        loop(test, name, socket)

      _ ->
        loop(test, name, socket)
    end
  end

  def send_as(pid, event, payload), do: send(pid, {:push, event, payload})

  def leave(pid, name) do
    send(pid, :leave)
    receive do: ({:left, ^name} -> :ok), after: (2000 -> raise "#{inspect(name)} didn't leave")
  end

  def framed(sub, frame), do: {:binary, <<byte_size(sub)::8, sub::binary, frame::binary>>}
end
