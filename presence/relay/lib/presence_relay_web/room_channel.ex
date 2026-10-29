defmodule PresenceRelayWeb.RoomChannel do
  @moduledoc """
  A room (PROTOCOL.md, "The room channel"): the token's room only. On
  join: presence, then every member's newest frame. Frames go to the rest
  of the room; `state` updates this connection's presence; waves and
  answers go to one member; reactions (an emoji) to the rest of the room;
  text (`say`) is kept (`PresenceRelay.Chat`) and goes to everyone, with
  the last page as `history` on join.
  """
  use Phoenix.Channel

  alias PresenceRelay.{Chat, Presence, Rooms}
  alias Phoenix.PubSub

  @modes ~w(avatar outline none)
  # A reaction: one emoji (a grapheme, no ASCII), at most this many bytes,
  # and at most one this often per connection.
  @react_bytes 32
  @react_ms 250
  # Text: at most this many messages per window per connection.
  @say_burst 5
  @say_window_ms 5_000

  @impl true
  def join("room:" <> room, _payload, %{assigns: %{room: room} = a} = socket) do
    case Rooms.join(a.app, room, a.sub, self()) do
      :ok ->
        for topic <- [
              Rooms.room_topic(a.app, room),
              Rooms.member_topic(a.app, room, a.sub),
              Rooms.presence_topic(a.app, room)
            ],
            do: PubSub.subscribe(PresenceRelay.PubSub, topic)

        send(self(), :after_join)
        {:ok, %{you: a.sub}, socket}

      {:error, reason} ->
        {:error, %{reason: reason}}
    end
  end

  def join("room:" <> _, _payload, _socket), do: {:error, %{reason: "not this token's room"}}

  @impl true
  def handle_info(:after_join, %{assigns: a} = socket) do
    meta = %{name: a.name, look: a.look, mode: "none", away: false, watching: true}
    {:ok, _} = Presence.track(self(), presence_topic(socket), a.sub, meta)
    push(socket, "presence_state", Presence.list(presence_topic(socket)))
    for frame <- Rooms.fresh(a.app, a.room), do: push(socket, "frame", {:binary, frame})
    push(socket, "history", %{messages: Chat.history(a.app, a.room)})
    {:noreply, socket}
  end

  def handle_info({:relay_said, msg}, socket) do
    push(socket, "said", msg)
    {:noreply, socket}
  end

  def handle_info({:relay_frame, frame}, socket) do
    push(socket, "frame", {:binary, frame})
    {:noreply, socket}
  end

  def handle_info({:relay_off, sub}, socket) do
    push(socket, "off", %{sub: sub})
    {:noreply, socket}
  end

  def handle_info({:relay_wave, from}, socket) do
    push(socket, "wave", %{from: from})
    {:noreply, socket}
  end

  def handle_info({:relay_react, from, emoji}, socket) do
    push(socket, "react", %{from: from, emoji: emoji})
    {:noreply, socket}
  end

  def handle_info({:relay_answer, from, yes}, socket) do
    push(socket, "answer", %{from: from, yes: yes})
    {:noreply, socket}
  end

  # Presence's diffs, on the room's internal presence topic.
  def handle_info(%Phoenix.Socket.Broadcast{event: "presence_diff", payload: diff}, socket) do
    push(socket, "presence_diff", diff)
    {:noreply, socket}
  end

  @impl true
  def handle_in("frame", {:binary, frame}, %{assigns: a} = socket) do
    # Too big or too soon: dropped without a word, as the protocol says.
    _ = Rooms.frame(a.app, a.room, a.sub, frame)
    {:noreply, socket}
  end

  def handle_in("frame", _not_binary, socket), do: {:noreply, socket}

  def handle_in("state", payload, %{assigns: a} = socket) when is_map(payload) do
    change = state(payload)

    if change != %{} do
      Presence.update(self(), presence_topic(socket), a.sub, &Map.merge(&1, change))
      if change[:mode] == "none", do: Rooms.camera_off(a.app, a.room, a.sub)
    end

    {:noreply, socket}
  end

  def handle_in("wave", %{"to" => to}, %{assigns: a} = socket) when is_binary(to) do
    PubSub.broadcast(
      PresenceRelay.PubSub,
      Rooms.member_topic(a.app, a.room, to),
      {:relay_wave, a.sub}
    )

    {:noreply, socket}
  end

  def handle_in("answer", %{"to" => to, "yes" => yes}, %{assigns: a} = socket)
      when is_binary(to) and is_boolean(yes) do
    PubSub.broadcast(
      PresenceRelay.PubSub,
      Rooms.member_topic(a.app, a.room, to),
      {:relay_answer, a.sub, yes}
    )

    {:noreply, socket}
  end

  def handle_in("react", %{"emoji" => emoji}, %{assigns: a} = socket) when is_binary(emoji) do
    now = System.monotonic_time(:millisecond)
    last = Map.get(a, :reacted_at)

    if emoji?(emoji) and (is_nil(last) or now - last >= @react_ms) do
      PubSub.broadcast_from(
        PresenceRelay.PubSub,
        self(),
        Rooms.room_topic(a.app, a.room),
        {:relay_react, a.sub, emoji}
      )

      {:noreply, assign(socket, :reacted_at, now)}
    else
      # Not an emoji, or too soon: dropped without a word.
      {:noreply, socket}
    end
  end

  def handle_in("say", %{"text" => text}, %{assigns: a} = socket) do
    now = System.monotonic_time(:millisecond)
    recent = Enum.filter(Map.get(a, :said_at, []), &(now - &1 < @say_window_ms))

    if length(recent) >= @say_burst do
      {:reply, {:error, %{reason: "too fast"}}, assign(socket, :said_at, recent)}
    else
      case Chat.say(a.app, a.room, a.sub, a.name, text) do
        {:ok, msg} ->
          # Everyone else, the sender's other connections included; this
          # connection hears it in the reply.
          PubSub.broadcast_from(
            PresenceRelay.PubSub,
            self(),
            Rooms.room_topic(a.app, a.room),
            {:relay_said, msg}
          )

          {:reply, {:ok, msg}, assign(socket, :said_at, [now | recent])}

        {:error, why} ->
          {:reply, {:error, %{reason: reason(why)}}, assign(socket, :said_at, recent)}
      end
    end
  end

  def handle_in("say", _payload, socket), do: {:reply, {:error, %{reason: "empty"}}, socket}

  def handle_in("history", payload, %{assigns: a} = socket) do
    before = if is_map(payload) and is_integer(payload["before"]), do: payload["before"]
    {:reply, {:ok, %{messages: Chat.history(a.app, a.room, before)}}, socket}
  end

  def handle_in(_event, _payload, socket), do: {:noreply, socket}

  defp reason(:empty), do: "empty"
  defp reason(:too_long), do: "too long"
  defp reason(:control), do: "control characters"
  defp reason(_), do: "not kept"

  defp emoji?(e) do
    byte_size(e) <= @react_bytes and String.valid?(e) and length(String.graphemes(e)) == 1 and
      e |> String.to_charlist() |> Enum.all?(&(&1 > 0x7F))
  end

  defp presence_topic(%{assigns: a}), do: Rooms.presence_topic(a.app, a.room)

  # The keys a client may set on its meta, each checked.
  defp state(p) do
    %{}
    |> put_if(:mode, p["mode"], &(&1 in @modes))
    |> put_if(:away, p["away"], &is_boolean/1)
    |> put_if(:watching, p["watching"], &is_boolean/1)
    |> put_look(p)
  end

  defp put_if(m, key, v, ok?), do: if(not is_nil(v) and ok?.(v), do: Map.put(m, key, v), else: m)

  defp put_look(m, %{"look" => look}) do
    case PresenceRelay.Token.look(look) do
      nil -> m
      l -> Map.put(m, :look, l)
    end
  end

  defp put_look(m, _), do: m
end
