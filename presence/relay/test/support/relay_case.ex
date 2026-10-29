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
  def client(name, room, sub, opts \\ []) do
    test = self()

    pid =
      spawn(fn ->
        tok =
          token(
            Map.merge(%{"room" => room, "sub" => sub, "name" => to_string(name)}, Map.new(opts))
          )

        {:ok, socket} =
          Phoenix.ChannelTest.__connect__(
            PresenceRelayWeb.Endpoint,
            PresenceRelayWeb.Socket,
            %{"token" => tok},
            test_process: test
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
      {:joined, ^name, reply} -> {:ok, pid, reply}
      {:refused, ^name, reply} -> {:error, reply}
    after
      2000 -> raise "#{name} didn't join"
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

      _ ->
        loop(test, name, socket)
    end
  end

  def send_as(pid, event, payload), do: send(pid, {:push, event, payload})

  def leave(pid, name) do
    send(pid, :leave)
    receive do: ({:left, ^name} -> :ok), after: (2000 -> raise "#{name} didn't leave")
  end

  def framed(sub, frame), do: {:binary, <<byte_size(sub)::8, sub::binary, frame::binary>>}
end
