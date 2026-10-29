defmodule PresenceRelay.Calls do
  @moduledoc """
  Calls on LiveKit (PROTOCOL.md, "Calls"; #318's P7 voice, P8 video). The
  relay never carries audio or video: it rings, hears the answer, and hands each party a
  short-lived LiveKit access token for one LiveKit room, named for the
  app, the room and the call (`<app>:<room>:<id>`), so two apps' calls (or
  two rooms') never meet.

  Calls are on when `LIVEKIT_URL`, `LIVEKIT_API_KEY` and
  `LIVEKIT_API_SECRET` are all set (config/runtime.exs); otherwise `call`
  is refused with "calls aren't set up here" and the join reply says
  `calls: false`, so a page hides its call buttons.

  Who's in a call is an OTP `:pg` group per call, `{:call, app, room, id}`,
  in the rooms' scope (four elements, so `PresenceRelay.Rooms`' member
  groups, three, never count it), so it spans the cluster like the rooms'
  connections do; and each member's presence meta says which call
  they're in (`call`), for tiles.

  ## The metering hook

  Every start, join, leave and end goes through `event/3`, a telemetry
  event `[:presence_relay, :call, kind]` (P9 meters call minutes from
  these; nothing here meters):

  - `:start`: a call began (its first two people are joining).
  - `:join`: one person joined it (`sub`).
  - `:leave`: one person left it, with `duration_ms` they were in it.
  - `:stop`: its last person left. (`:pg` is eventually consistent: two
    people leaving at the same instant on two nodes could both, or
    neither, see themselves as the last. The `:leave`s are exact.)
  """

  # A LiveKit token's life. LiveKit checks it on connecting only (a call
  # outlives it) and refreshes a connected participant's itself; a page
  # that drops out asks for a fresh one (`call_token`).
  @ttl 600
  @scope :presence_relay

  @doc "LiveKit's `%{url, key, secret}`, or nil when calls are off."
  def config do
    case Application.get_env(:presence_relay, :livekit) do
      %{url: url, key: key, secret: secret} = c
      when is_binary(url) and is_binary(key) and is_binary(secret) ->
        c

      _ ->
        nil
    end
  end

  def on?, do: config() != nil

  @doc "A new call's id: 16 url-safe characters."
  def new_id, do: Base.url_encode64(:crypto.strong_rand_bytes(12), padding: false)

  @doc "The LiveKit room a call happens in."
  def livekit_room(app, room, id), do: "#{app}:#{room}:#{id}"

  @doc """
  What a page needs to join call `id` as `sub`: LiveKit's URL, a token
  and the LiveKit room's name. The token lets it publish its microphone
  and its camera (P8's video, which the page turns on), and nothing else:
  no screen, no data.
  """
  def join_info(app, room, id, sub, name, now \\ System.system_time(:second)) do
    %{url: url} = c = config()
    lk_room = livekit_room(app, room, id)
    %{id: id, url: url, room: lk_room, token: token(c, lk_room, sub, name, now)}
  end

  @doc """
  A LiveKit access token (docs.livekit.io, "Authentication"): an HS256
  JWT signed with the API secret, `iss` the API key, `sub` the identity
  (the member's sub), `name`, and a video grant for this room only.
  """
  def token(%{key: key, secret: secret}, lk_room, sub, name, now) do
    claims = %{
      "iss" => key,
      "sub" => sub,
      "name" => name,
      "nbf" => now,
      "exp" => now + @ttl,
      "video" => %{
        "room" => lk_room,
        "roomJoin" => true,
        "canPublish" => true,
        "canPublishSources" => ["microphone", "camera"],
        "canPublishData" => false,
        "canSubscribe" => true
      }
    }

    h = encode(%{"alg" => "HS256", "typ" => "JWT"})
    p = encode(claims)

    h <>
      "." <>
      p <>
      "." <> Base.url_encode64(:crypto.mac(:hmac, :sha256, secret, h <> "." <> p), padding: false)
  end

  defp encode(map), do: Base.url_encode64(Jason.encode!(map), padding: false)

  @doc "`pid` (a connection) is in call `id` until it leaves or exits."
  def join(app, room, id, sub, pid \\ self()) do
    :ok = :pg.join(@scope, group(app, room, id), pid)
    event(:join, %{}, %{app: app, room: room, call: id, sub: sub})
  end

  @doc """
  `pid` left call `id` after `ms`; the call's `:stop` too if nobody's
  left in it.
  """
  def leave(app, room, id, sub, ms, pid \\ self()) do
    g = group(app, room, id)
    :pg.leave(@scope, g, pid)
    meta = %{app: app, room: room, call: id}
    event(:leave, %{duration_ms: ms}, Map.put(meta, :sub, sub))
    if :pg.get_members(@scope, g) -- [pid] == [], do: event(:stop, %{}, meta)
    :ok
  end

  @doc "A call began: its first two people are about to join it."
  def started(app, room, id), do: event(:start, %{}, %{app: app, room: room, call: id})

  @doc "How many connections are in call `id`, across the cluster."
  def size(app, room, id), do: length(:pg.get_members(@scope, group(app, room, id)))

  defp group(app, room, id), do: {:call, app, room, id}

  # The one place a call's life is told: start, join, leave, stop.
  defp event(kind, measurements, meta) do
    :telemetry.execute(
      [:presence_relay, :call, kind],
      Map.put(measurements, :system_time, System.system_time(:millisecond)),
      meta
    )
  end
end
