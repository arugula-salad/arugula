defmodule PresenceRelayWeb.Socket do
  @moduledoc """
  A client's socket (PROTOCOL.md, "Connecting"), admitted one of two ways:

  - `token`: a room token (a host app's server signed it, or the demo's).
  - `key` + `room`: a publishable key (#318 P3), from a page on one of its
    app's allowed origins (a test key also from localhost), and then
    `name` for a guest, where the app allows guests. (A provider token,
    verified against the app's issuer, comes with P4.)

  Either way the socket carries the same assigns (app, room, sub, name,
  look) for the room channel.
  """
  use Phoenix.Socket

  alias PresenceRelay.{Apps, Token}
  alias PresenceRelayWeb.Origin

  channel "room:*", PresenceRelayWeb.RoomChannel

  @impl true
  def connect(params, socket, info) do
    # The noted Origin: taken first, whichever way in, so none goes stale.
    origin = Map.get(info, :origin) || Origin.take()

    case admit(params, origin) do
      {:ok, a} -> {:ok, assign(socket, a)}
      {:error, _} -> :error
    end
  end

  @impl true
  def id(_), do: nil

  @doc false
  def admit(%{"token" => token}, _origin) do
    with {:ok, t} <- Token.verify(token),
         do: {:ok, %{app: t.app, room: t.room, sub: t.sub, name: t.name, look: t.look}}
  end

  def admit(%{"key" => key, "room" => room} = p, origin) do
    with %{app: app, mode: mode} <- Apps.by_key(key) || {:error, :unknown_key},
         true <-
           Apps.origin_allowed?(Enum.map(app.origins, & &1.origin), mode, origin) ||
             {:error, :origin},
         true <- Token.valid_room?(room) || {:error, :bad_room} do
      guest(app, room, p)
    end
  end

  def admit(_, _), do: {:error, :no_credentials}

  defp guest(app, room, %{"name" => name} = p) do
    cond do
      not app.guests -> {:error, :no_guests}
      not Token.valid_name?(name) -> {:error, :bad_name}
      true -> {:ok, %{app: app.id, room: room, sub: "g_" <> rand(), name: name, look: look(p)}}
    end
  end

  defp guest(_, _, _), do: {:error, :no_identity}

  # `look` as `character:colour`, the query string's way.
  defp look(%{"look" => l}) when is_binary(l) do
    with [ch, co] <- String.split(l, ":"),
         {ch, ""} <- Integer.parse(ch),
         {co, ""} <- Integer.parse(co),
         do: Token.look(%{"character" => ch, "colour" => co}),
         else: (_ -> nil)
  end

  defp look(_), do: nil

  defp rand, do: Base.encode16(:crypto.strong_rand_bytes(6), case: :lower)
end
