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

  ## In a cluster (#318's P2)

  Members of one room may be connected to different nodes. PubSub and
  Presence already span the cluster, so frames, waves, answers, reactions
  and presence reach everyone wherever they're connected. What this module
  keeps is made cluster-wide like so:

  - **Connections** are an OTP `:pg` group per member, `{app, room, sub}`,
    in the `:presence_relay` scope: every node sees every connection, so
    the caps (64 members a room, 8 connections a member) count the whole
    cluster, and a member's camera is off only when their last connection
    anywhere is gone. (`:pg` is eventually consistent: two nodes admitting
    the 65th member at the same instant could both succeed. A soft cap.)
  - **Newest frames** stay in the ETS table of the node the member sent
    them through, and that node alone sweeps them, so a stale frame says
    `off` once. A joiner asks every node for its fresh frames (`fresh/2`);
    forgetting a frame (`camera_off`) forgets it on every node.
  - **Rate limits** stay where they are: per connection, on its node.
  """
  use GenServer

  alias Phoenix.PubSub

  @table :presence_relay_frames
  @scope :presence_relay
  @doc "The `:pg` scope connections are grouped in (started by the application)."
  def scope, do: @scope
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

  @doc """
  Every member's newest frame younger than the stale age, prefixed, from
  every node in the cluster (each keeps the frames sent through it).
  """
  def fresh(app, room) do
    others =
      Node.list()
      |> :erpc.multicall(__MODULE__, :local_fresh, [app, room], 1_000)
      |> Enum.flat_map(fn
        {:ok, frames} -> frames
        _ -> []
      end)

    local_fresh(app, room) ++ others
  end

  @doc false
  # This node's fresh frames for the room (fresh/2 asks every node).
  def local_fresh(app, room) do
    cutoff = System.monotonic_time(:millisecond) - stale_ms()

    for {{_, _, sub}, at, frame} <- :ets.match_object(@table, {{app, room, :_}, :_, :_}),
        at >= cutoff,
        do: prefixed(sub, frame)
  end

  @doc "`sub`'s camera went off: forget their frame (on every node) and tell the room (but `from`)."
  def camera_off(app, room, sub, from \\ self()) do
    key = {app, room, sub}
    :ets.delete(@table, key)
    :erpc.multicast(Node.list(), __MODULE__, :forget, [key])
    PubSub.broadcast_from(PresenceRelay.PubSub, from, room_topic(app, room), {:relay_off, sub})
  end

  @doc false
  # Another node's camera_off: drop this node's copy of the member's frame.
  def forget(key), do: :ets.delete(@table, key)

  @doc "How many members the room has, and `sub`'s connections, across the cluster."
  def count(app, room, sub) do
    members =
      @scope
      |> :pg.which_groups()
      |> Enum.count(&match?({^app, ^room, _}, &1))

    {members, length(:pg.get_members(@scope, {app, room, sub}))}
  end

  @doc "Whether `sub` has a connection in the room, anywhere in the cluster."
  def here?(app, room, sub), do: :pg.get_members(@scope, {app, room, sub}) != []

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
    # refs: monitor => {app, room, sub, pid}, for this node's connections.
    {:ok, %{refs: %{}}}
  end

  @impl true
  def handle_call({:join, {app, r}, sub, pid}, _from, s) do
    {members, n} = count(app, r, sub)

    cond do
      n == 0 and members >= max_members() ->
        {:reply, {:error, "room full"}, s}

      n >= max_conns() ->
        {:reply, {:error, "too many connections"}, s}

      true ->
        :ok = :pg.join(@scope, {app, r, sub}, pid)
        ref = Process.monitor(pid)
        {:reply, :ok, %{s | refs: Map.put(s.refs, ref, {app, r, sub, pid})}}
    end
  end

  @impl true
  def handle_info({:DOWN, ref, :process, _pid, _}, s) do
    case Map.pop(s.refs, ref) do
      {nil, _} ->
        {:noreply, s}

      {{app, r, sub, pid}, refs} ->
        # :pg drops a dead process by itself, but maybe not yet: leave
        # it out here. Their last connection anywhere: camera off.
        if :pg.get_members(@scope, {app, r, sub}) -- [pid] == [] do
          camera_off(app, r, sub, self())
        end

        {:noreply, %{s | refs: refs}}
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
