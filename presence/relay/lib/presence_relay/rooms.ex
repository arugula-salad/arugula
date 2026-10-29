defmodule PresenceRelay.Rooms do
  @moduledoc """
  The rooms' bookkeeping (PROTOCOL.md, "Limits" and the `off` event):
  each member's connections, admitted under the room's caps and watched
  until they go; each member's newest frame, kept in memory (ETS) for a
  joiner and nothing else; and the sweeps that say `off` for a camera whose
  frames stopped.

  `off` goes to the room when a member's camera goes off (`camera_off/4`),
  when their newest frame goes stale, and when their last connection
  leaves. Internal topics carry the app, so two apps' rooms of one name
  never meet: `r:<app>:<room>` (frames and `off`), `m:<app>:<room>:<sub>`
  (waves and answers for one member), `p:<app>:<room>` (presence).
  """
  use GenServer

  alias Phoenix.PubSub

  @table :presence_relay_frames
  @max_frame 32 * 1024
  @gap_ms 70

  def start_link(_), do: GenServer.start_link(__MODULE__, nil, name: __MODULE__)

  def room_topic(app, room), do: "r:#{app}:#{room}"
  def member_topic(app, room, sub), do: "m:#{app}:#{room}:#{sub}"
  def presence_topic(app, room), do: "p:#{app}:#{room}"

  @doc """
  Admits `pid` as one of `sub`'s connections to the room, or refuses it:
  a room holds 64 members, a member 8 connections. The connection counts
  until its process exits.
  """
  def join(app, room, sub, pid), do: GenServer.call(__MODULE__, {:join, {app, room}, sub, pid})

  @doc """
  Keeps `frame` as `sub`'s newest and passes it to the rest of the room,
  prefixed with who sent it; `:drop` for one too big, or too soon after
  the member's last (70 ms).
  """
  def frame(app, room, sub, frame, from \\ self())

  def frame(_app, _room, _sub, frame, _from) when byte_size(frame) > @max_frame or frame == "",
    do: :drop

  def frame(app, room, sub, frame, from) do
    now = System.monotonic_time(:millisecond)
    key = {app, room, sub}

    case :ets.lookup(@table, key) do
      [{^key, at, _}] when now - at < @gap_ms ->
        :drop

      _ ->
        :ets.insert(@table, {key, now, frame})

        PubSub.broadcast_from(
          PresenceRelay.PubSub,
          from,
          room_topic(app, room),
          {:relay_frame, prefixed(sub, frame)}
        )

        :ok
    end
  end

  @doc "Every member's newest frame younger than the stale age, prefixed."
  def fresh(app, room) do
    cutoff = System.monotonic_time(:millisecond) - stale_ms()

    for {{_, _, sub}, at, frame} <- :ets.match_object(@table, {{app, room, :_}, :_, :_}),
        at >= cutoff,
        do: prefixed(sub, frame)
  end

  @doc "`sub`'s camera went off: forget their frame and tell the room (but `from`)."
  def camera_off(app, room, sub, from \\ self()) do
    :ets.delete(@table, {app, room, sub})
    PubSub.broadcast_from(PresenceRelay.PubSub, from, room_topic(app, room), {:relay_off, sub})
  end

  defp prefixed(sub, frame), do: <<byte_size(sub)::8, sub::binary, frame::binary>>

  defp stale_ms, do: Application.get_env(:presence_relay, :stale_ms, 10_000)
  defp sweep_ms, do: Application.get_env(:presence_relay, :sweep_ms, 2_000)
  defp max_members, do: Application.get_env(:presence_relay, :max_members, 64)
  defp max_conns, do: Application.get_env(:presence_relay, :max_conns, 8)

  @impl true
  def init(_) do
    :ets.new(@table, [
      :named_table,
      :public,
      :set,
      read_concurrency: true,
      write_concurrency: true
    ])

    Process.send_after(self(), :sweep, sweep_ms())
    # conns: {app, room} => %{sub => count}; refs: monitor => {app, room, sub}
    {:ok, %{conns: %{}, refs: %{}}}
  end

  @impl true
  def handle_call({:join, room, sub, pid}, _from, s) do
    members = Map.get(s.conns, room, %{})
    n = Map.get(members, sub, 0)

    cond do
      n == 0 and map_size(members) >= max_members() ->
        {:reply, {:error, "room full"}, s}

      n >= max_conns() ->
        {:reply, {:error, "too many connections"}, s}

      true ->
        ref = Process.monitor(pid)
        {app, r} = room

        s = %{
          s
          | conns: Map.put(s.conns, room, Map.put(members, sub, n + 1)),
            refs: Map.put(s.refs, ref, {app, r, sub})
        }

        {:reply, :ok, s}
    end
  end

  @impl true
  def handle_info({:DOWN, ref, :process, _pid, _}, s) do
    case Map.pop(s.refs, ref) do
      {nil, _} ->
        {:noreply, s}

      {{app, r, sub}, refs} ->
        room = {app, r}
        members = Map.get(s.conns, room, %{})

        members =
          case Map.get(members, sub, 1) - 1 do
            0 ->
              # Their last connection: their camera's off for the room.
              camera_off(app, r, sub, self())
              Map.delete(members, sub)

            n ->
              Map.put(members, sub, n)
          end

        conns =
          if members == %{}, do: Map.delete(s.conns, room), else: Map.put(s.conns, room, members)

        {:noreply, %{s | conns: conns, refs: refs}}
    end
  end

  def handle_info(:sweep, s) do
    cutoff = System.monotonic_time(:millisecond) - stale_ms()
    stale = :ets.select(@table, [{{:"$1", :"$2", :_}, [{:<, :"$2", cutoff}], [:"$1"]}])
    for {app, r, sub} <- stale, do: camera_off(app, r, sub, self())
    Process.send_after(self(), :sweep, sweep_ms())
    {:noreply, s}
  end
end
