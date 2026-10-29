defmodule PresenceRelayWeb.Socket do
  @moduledoc """
  A client's socket (PROTOCOL.md, "Connecting"), admitted one of two ways:

  - `token`: a room token (a host app's server signed it, or the demo's).
  - `key` + `room`: a publishable key (#318 P3), from a page on one of its
    app's allowed origins (a test key also from localhost), and then
    either `auth`, the end user's own token from the app's sign-in
    provider (P4: `PresenceRelay.Issuers` checks it), for a member; or
    `name` for a guest, where the app allows guests, and optionally `gid`,
    the browser's own random id, which metering counts the guest by
    (P9, `PresenceRelay.Usage`) and nothing else trusts.

  Either way the socket carries the same assigns (app, room, sub, name,
  look, and `meter`, whom usage counts) for the room channel.
  """
  use Phoenix.Socket

  alias PresenceRelay.{Apps, Issuers, Token, Usage}
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
         do:
           {:ok,
            %{app: t.app, room: t.room, sub: t.sub, name: t.name, look: t.look, meter: t.sub}}
  end

  def admit(%{"key" => key, "room" => room} = p, origin) do
    with %{app: app, mode: mode} <- Apps.by_key(key) || {:error, :unknown_key},
         true <-
           Apps.origin_allowed?(Enum.map(app.origins, & &1.origin), mode, origin) ||
             {:error, :origin},
         true <- Token.valid_room?(room) || {:error, :bad_room} do
      case p do
        %{"auth" => auth} when is_binary(auth) and auth != "" ->
          member(app, room, auth, origin, p)

        _ ->
          guest(app, room, p)
      end
    end
  end

  def admit(_, _), do: {:error, :no_credentials}

  defp guest(app, room, %{"name" => name} = p) do
    cond do
      not app.guests -> {:error, :no_guests}
      not Token.valid_name?(name) -> {:error, :bad_name}
      true -> {:ok, guest_assigns(app, room, name, p)}
    end
  end

  defp guest(_, _, _), do: {:error, :no_identity}

  # A guest's sub is new for each socket; their `gid`, if sound, is what
  # metering counts them by (PresenceRelay.Usage), and nothing else.
  defp guest_assigns(app, room, name, p) do
    sub = "g_" <> rand()

    %{
      app: app.id,
      room: room,
      sub: sub,
      name: name,
      look: look(p),
      meter: Usage.guest_key(sub, p)
    }
  end

  # A signed-in member: their provider's token, checked against the app's
  # issuer (and its room rule); `name` only stands in for a token without one.
  defp member(%{issuer: nil}, _, _, _, _), do: {:error, :no_issuer}

  defp member(app, room, auth, origin, p) do
    with {:ok, m} <-
           Issuers.verify(app.issuer, auth, origin: origin, room: room, name: p["name"]),
         do:
           {:ok,
            %{app: app.id, room: room, sub: m.sub, name: m.name, look: look(p), meter: m.sub}}
  end

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
