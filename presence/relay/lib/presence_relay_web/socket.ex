defmodule PresenceRelayWeb.Socket do
  @moduledoc """
  A client's socket (PROTOCOL.md, "Connecting"): admitted by its room
  token, whose claims it carries for the room channel.
  """
  use Phoenix.Socket

  channel "room:*", PresenceRelayWeb.RoomChannel

  @impl true
  def connect(%{"token" => token}, socket, _info) do
    case PresenceRelay.Token.verify(token) do
      {:ok, t} ->
        {:ok, assign(socket, app: t.app, room: t.room, sub: t.sub, name: t.name, look: t.look)}

      {:error, _} ->
        :error
    end
  end

  def connect(_, _, _), do: :error

  @impl true
  def id(_), do: nil
end
