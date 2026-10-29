defmodule PresenceRelayWeb.RoomChannel do
  @moduledoc """
  A room (PROTOCOL.md, "The room channel"): the token's room only. On
  join: presence, then every member's newest frame. Frames go to the rest
  of the room; `state` updates this connection's presence; waves and
  answers go to one member; reactions (an emoji) to the rest of the room;
  text (`say`) is kept (`PresenceRelay.Chat`) and goes to everyone, with
  the last page as `history` on join. Calls (`PresenceRelay.Calls`) ring
  one member, and on a yes hand both connections a LiveKit token; see
  "Calls" below.
  """
  use Phoenix.Channel

  alias PresenceRelay.{Calls, Chat, Presence, Rooms}
  alias Phoenix.PubSub

  @modes ~w(avatar outline none)
  # A reaction: one emoji (a grapheme, no ASCII), at most this many bytes,
  # and at most one this often per connection.
  @react_bytes 32
  @react_ms 250
  # Text: at most this many messages per window per connection.
  @say_burst 5
  @say_window_ms 5_000
  # Calls: at most this many rings per window per connection.
  @call_burst 3
  @call_window_ms 30_000

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
        # A connection's calls: the rings it made and is waiting on
        # (id => callee), the rings it may answer (id => caller), and the
        # call it's in ({id, joined at}).
        socket = assign(socket, ringing: %{}, rung: %{}, in_call: nil, called_at: [])
        {:ok, %{you: a.sub, calls: Calls.on?()}, socket}

      {:error, reason} ->
        {:error, %{reason: reason}}
    end
  end

  def join("room:" <> _, _payload, _socket), do: {:error, %{reason: "not this token's room"}}

  @impl true
  def handle_info(:after_join, %{assigns: a} = socket) do
    meta = %{name: a.name, look: a.look, mode: "none", away: false, watching: true, call: nil}
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

  # Calls. Someone rings this member: any of its connections may answer.
  def handle_info({:relay_call, from, id}, socket) do
    push(socket, "call", %{from: from, id: id})
    {:noreply, change(socket, :rung, &Map.put(&1, id, from))}
  end

  # A ring this member could answer is over: answered on another of its
  # pages, given up by the caller, or not answered in time.
  def handle_info({:relay_call_over, id}, %{assigns: a} = socket) do
    if Map.has_key?(a.rung, id) do
      push(socket, "call_cancel", %{id: id})
      {:noreply, change(socket, :rung, &Map.delete(&1, id))}
    else
      {:noreply, socket}
    end
  end

  # The answer to a ring this connection made. On a yes it joins (a new
  # call starts here) and tells the answering connection to join too.
  def handle_info({:relay_call_answer, id, from, yes, answerer}, %{assigns: a} = socket) do
    case Map.pop(a.ringing, id) do
      {nil, _} ->
        {:noreply, socket}

      {_to, ringing} ->
        push(socket, "call_answer", %{id: id, from: from, yes: yes})
        socket = assign(socket, :ringing, ringing)

        socket =
          cond do
            not yes ->
              socket

            match?({^id, _}, a.in_call) ->
              send(answerer, {:relay_call_go, id})
              socket

            true ->
              Calls.started(a.app, a.room, id)
              socket = join_call(socket, id)
              send(answerer, {:relay_call_go, id})
              socket
          end

        {:noreply, socket}
    end
  end

  # The caller has the yes: this (answering) connection joins.
  def handle_info({:relay_call_go, id}, socket), do: {:noreply, join_call(socket, id)}

  # A ring nobody answered in time.
  def handle_info({:ring_over, id}, %{assigns: a} = socket) do
    case Map.pop(a.ringing, id) do
      {nil, _} ->
        {:noreply, socket}

      {to, ringing} ->
        PubSub.broadcast(
          PresenceRelay.PubSub,
          Rooms.member_topic(a.app, a.room, to),
          {:relay_call_over, id}
        )

        push(socket, "call_answer", %{id: id, from: to, yes: false, reason: "no answer"})
        {:noreply, assign(socket, :ringing, ringing)}
    end
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

  # Calls (PROTOCOL.md, "Calls").
  def handle_in("call", %{"to" => to}, %{assigns: a} = socket) when is_binary(to) do
    now = System.monotonic_time(:millisecond)
    recent = Enum.filter(a.called_at, &(now - &1 < @call_window_ms))
    # Already in a call: the ring asks them into it (a call grows).
    id = with {id, _} <- a.in_call, do: id

    why =
      cond do
        not Calls.on?() -> "calls aren't set up here"
        to == a.sub -> "that's you"
        not Rooms.here?(a.app, a.room, to) -> "not in the room"
        id && in_call?(socket, to, id) -> "already in the call"
        length(recent) >= @call_burst -> "too fast"
        true -> nil
      end

    if why do
      {:reply, {:error, %{reason: why}}, socket}
    else
      id = id || Calls.new_id()

      PubSub.broadcast(
        PresenceRelay.PubSub,
        Rooms.member_topic(a.app, a.room, to),
        {:relay_call, a.sub, id}
      )

      Process.send_after(self(), {:ring_over, id}, ring_ms())

      socket =
        socket
        |> change(:ringing, &Map.put(&1, id, to))
        |> assign(:called_at, [now | recent])

      {:reply, {:ok, %{id: id}}, socket}
    end
  end

  def handle_in("call", _payload, socket), do: {:reply, {:error, %{reason: "no one"}}, socket}

  def handle_in("call_answer", %{"id" => id, "yes" => yes}, %{assigns: a} = socket)
      when is_binary(id) and is_boolean(yes) do
    case Map.pop(a.rung, id) do
      {nil, _} ->
        {:reply, {:error, %{reason: "no such call"}}, socket}

      {from, rung} ->
        # This member's other pages stop ringing.
        PubSub.broadcast_from(
          PresenceRelay.PubSub,
          self(),
          Rooms.member_topic(a.app, a.room, a.sub),
          {:relay_call_over, id}
        )

        PubSub.broadcast(
          PresenceRelay.PubSub,
          Rooms.member_topic(a.app, a.room, from),
          {:relay_call_answer, id, a.sub, yes, self()}
        )

        {:reply, :ok, assign(socket, :rung, rung)}
    end
  end

  def handle_in("call_leave", %{"id" => id}, socket) when is_binary(id) do
    socket = socket |> cancel_ring(id) |> leave_call(id)
    {:reply, :ok, socket}
  end

  def handle_in("call_token", %{"id" => id}, %{assigns: a} = socket) when is_binary(id) do
    case a.in_call do
      {^id, _} -> {:reply, {:ok, Calls.join_info(a.app, a.room, id, a.sub, a.name)}, socket}
      _ -> {:reply, {:error, %{reason: "not in that call"}}, socket}
    end
  end

  def handle_in(_event, _payload, socket), do: {:noreply, socket}

  # A connection that goes leaves its call and gives up its rings.
  @impl true
  def terminate(_reason, %{assigns: %{ringing: ringing, in_call: in_call}} = socket) do
    socket = Enum.reduce(Map.keys(ringing), socket, &cancel_ring(&2, &1))
    with {id, _} <- in_call, do: leave_call(socket, id, false)
    :ok
  end

  def terminate(_reason, _socket), do: :ok

  defp change(socket, key, fun), do: assign(socket, key, fun.(socket.assigns[key]))

  defp ring_ms, do: Application.get_env(:presence_relay, :ring_ms, 30_000)

  # Join call `id` (leaving any other first): presence says so, and the
  # page gets its LiveKit token.
  defp join_call(%{assigns: a} = socket, id) do
    socket =
      case a.in_call do
        {^id, _} -> socket
        {other, _} -> leave_call(socket, other)
        nil -> socket
      end

    Calls.join(a.app, a.room, id, a.sub)
    Presence.update(self(), presence_topic(socket), a.sub, &Map.put(&1, :call, id))
    push(socket, "call_join", Calls.join_info(a.app, a.room, id, a.sub, a.name))
    assign(socket, :in_call, {id, System.monotonic_time(:millisecond)})
  end

  # Leave call `id` if this connection is in it (`tell`: update presence,
  # which a connection that's going doesn't need).
  defp leave_call(%{assigns: a} = socket, id, tell \\ true) do
    case a.in_call do
      {^id, at} ->
        Calls.leave(a.app, a.room, id, a.sub, System.monotonic_time(:millisecond) - at)

        if tell,
          do: Presence.update(self(), presence_topic(socket), a.sub, &Map.put(&1, :call, nil))

        assign(socket, :in_call, nil)

      _ ->
        socket
    end
  end

  # Give up a ring this connection made: the callee's pages stop ringing.
  defp cancel_ring(%{assigns: a} = socket, id) do
    case Map.pop(a.ringing, id) do
      {nil, _} ->
        socket

      {to, ringing} ->
        PubSub.broadcast(
          PresenceRelay.PubSub,
          Rooms.member_topic(a.app, a.room, to),
          {:relay_call_over, id}
        )

        assign(socket, :ringing, ringing)
    end
  end

  # Whether any of `sub`'s connections is in call `id` (presence says).
  defp in_call?(socket, sub, id) do
    case Presence.get_by_key(presence_topic(socket), sub) do
      %{metas: metas} -> Enum.any?(metas, &(&1[:call] == id))
      _ -> false
    end
  end

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
