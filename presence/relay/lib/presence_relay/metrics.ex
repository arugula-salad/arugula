defmodule PresenceRelay.Metrics do
  @moduledoc """
  The relay's metrics (#318's P12), in Prometheus' text format at
  `/metrics` on an internal port (`PresenceRelayWeb.MetricsPlug`), which
  Fly scrapes (the `[metrics]` section of presence/fly.toml) into its
  managed Grafana.

  A small exporter of our own rather than telemetry_metrics and a
  Prometheus reporter: what we count already arrives as `:telemetry`
  events (ours, Phoenix's, Ecto's), half of what we show is read at
  scrape time (rooms and calls from `:pg`, the pool, the BEAM), and a
  counter is one atomic `:ets.update_counter/4`. That's this module, with
  no deps to keep up.

  ## What's counted

  Counters and histograms, from telemetry, kept in ETS until the node
  stops (Prometheus takes rates of them):

  - `presence_socket_connects_total{result}`: sockets admitted or refused.
  - `presence_joins_total`, and `presence_joins_refused_total{reason}`:
    `token`, `origin`, `guest` and `rate` at connect, `cap` (the app's
    monthly limit), `room_full`, `connections` (a member's),
    `app_connections` and `wrong_room` at join.
  - `presence_frames_total{result}`: frames in, `relayed`, `too_big` or
    `too_soon` (rate: frames a second); `presence_frame_bytes_total`.
  - `presence_messages_total{kind, result}`: say, react, wave, answer.
  - `presence_calls_started_total`; `presence_call_seconds_total`, each
    person's time in calls (over 60: call minutes).
  - `presence_rate_limited_total{limit}`: hits over an abuse limit.
  - `presence_http_requests_total{status}` (by class: `2xx`…) and
    `presence_http_request_seconds`.
  - `presence_db_query_seconds` and `presence_db_queue_seconds` (Ecto).
  - `presence_usage_flush_seconds`, the usage buffer's flushes.

  Gauges, read at scrape: `presence_connections` (this node's: a
  dashboard sums them), `presence_rooms`, `presence_members`,
  `presence_calls_active`, `presence_call_participants` (the cluster's,
  which every node sees alike: a dashboard takes the max),
  `presence_cluster_nodes`, `presence_usage_unflushed_seconds` (since a
  flush last left nothing behind; it grows while Postgres refuses
  writes) and `presence_usage_kept`, `presence_db_pool_size`,
  `presence_db_pool_ready` and `presence_db_pool_queue`, and the BEAM's
  basics (`presence_beam_*`; `presence_beam_scheduler_utilization` is the
  share of the schedulers' time spent working since the last scrape).

  Labels are small fixed sets: no app, room or person is ever a label.
  """
  use GenServer

  alias PresenceRelay.Rooms

  @table :presence_relay_metrics

  # Seconds.
  @fast [0.001, 0.0025, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1, 2.5, 5]

  # name => {type, help, options}: the counters and histograms, in the
  # order they're written.
  @defs [
    presence_socket_connects_total: {:counter, "Sockets admitted or refused.", []},
    presence_joins_total: {:counter, "Room joins admitted.", []},
    presence_joins_refused_total:
      {:counter, "Connects and joins refused, by reason (token, origin, guest, rate, cap, …).",
       []},
    presence_frames_total:
      {:counter, "Frames in: relayed, or dropped as too_big or too_soon.", []},
    presence_frame_bytes_total: {:counter, "Bytes of frames relayed (before fan-out).", []},
    presence_messages_total:
      {:counter, "Room messages (say, react, wave, answer), kept or dropped.", []},
    presence_calls_started_total: {:counter, "Calls started.", []},
    presence_call_seconds_total:
      {:counter, "Each person's time in calls, summed (over 60: call minutes).", scale: 1000},
    presence_rate_limited_total: {:counter, "Hits over an abuse limit, by limit.", []},
    presence_http_requests_total: {:counter, "HTTP requests, by status class.", []},
    presence_http_request_seconds: {:histogram, "HTTP request time.", buckets: @fast},
    presence_db_query_seconds: {:histogram, "Postgres query and decode time.", buckets: @fast},
    presence_db_queue_seconds:
      {:histogram, "Time waiting for a pooled Postgres connection.", buckets: @fast},
    presence_usage_flush_seconds:
      {:histogram, "Usage buffer flushes' time.", buckets: [0.01, 0.05, 0.1, 0.5, 1, 5, 30]}
  ]

  @events [
    [:presence_relay, :socket, :connect],
    [:presence_relay, :join, :ok],
    [:presence_relay, :join, :refused],
    [:presence_relay, :frame],
    [:presence_relay, :message],
    [:presence_relay, :call, :start],
    [:presence_relay, :call, :leave],
    [:presence_relay, :limit, :refused],
    [:presence_relay, :usage, :flush],
    [:presence_relay, :repo, :query],
    [:phoenix, :endpoint, :stop]
  ]

  def start_link(_), do: GenServer.start_link(__MODULE__, nil, name: __MODULE__)

  @doc "Adds `by` to counter `name` with `labels` (a keyword list)."
  def inc(name, labels \\ [], by \\ 1) do
    k = {name, labels}
    :ets.update_counter(@table, k, {2, by}, {k, 0})
    :ok
  rescue
    ArgumentError -> :ok
  end

  @doc "Puts one observation, in seconds, in histogram `name`."
  def observe(name, labels, seconds) do
    {:histogram, _, o} = Keyword.fetch!(@defs, name)
    buckets = o[:buckets]
    i = Enum.find_index(buckets, &(seconds <= &1)) || length(buckets)
    k = {name, labels}
    default = List.to_tuple([k, 0, 0 | List.duplicate(0, length(buckets) + 1)])
    # count, sum (microseconds), and the one bucket it falls in.
    ops = [{2, 1}, {3, round(seconds * 1_000_000)}, {4 + i, 1}]
    :ets.update_counter(@table, k, ops, default)
    :ok
  rescue
    ArgumentError -> :ok
  end

  @doc "Seconds since a usage flush last left nothing to try again."
  def usage_unflushed_seconds do
    case :ets.lookup(@table, :usage_clean_at) do
      [{_, at}] -> (System.monotonic_time(:millisecond) - at) / 1000
      [] -> 0.0
    end
  rescue
    ArgumentError -> 0.0
  end

  @doc false
  # Tests: forget every count.
  def reset do
    :ets.delete_all_objects(@table)
    :ets.insert(@table, {:usage_clean_at, System.monotonic_time(:millisecond)})
    :ok
  end

  ## Telemetry

  @doc false
  def handle_event([:presence_relay, :socket, :connect], _, %{result: r}, _),
    do: inc(:presence_socket_connects_total, result: r)

  def handle_event([:presence_relay, :join, :ok], _, _, _), do: inc(:presence_joins_total)

  def handle_event([:presence_relay, :join, :refused], _, %{reason: r}, _),
    do: inc(:presence_joins_refused_total, reason: r)

  def handle_event([:presence_relay, :frame], m, %{result: r}, _) do
    inc(:presence_frames_total, result: r)
    if r == :relayed, do: inc(:presence_frame_bytes_total, [], m.bytes)
  end

  def handle_event([:presence_relay, :message], _, %{kind: k, result: r}, _),
    do: inc(:presence_messages_total, kind: k, result: r)

  def handle_event([:presence_relay, :call, :start], _, _, _),
    do: inc(:presence_calls_started_total)

  def handle_event([:presence_relay, :call, :leave], %{duration_ms: ms}, _, _)
      when is_integer(ms) and ms > 0,
      do: inc(:presence_call_seconds_total, [], ms)

  def handle_event([:presence_relay, :limit, :refused], _, %{limit: l}, _),
    do: inc(:presence_rate_limited_total, limit: l)

  def handle_event([:presence_relay, :usage, :flush], m, _, _) do
    observe(:presence_usage_flush_seconds, [], seconds(m.duration))
    :ets.insert(@table, {:usage_kept, m.kept})

    if m.kept == 0,
      do: :ets.insert(@table, {:usage_clean_at, System.monotonic_time(:millisecond)})
  end

  def handle_event([:presence_relay, :repo, :query], m, _, _) do
    query = (m[:query_time] || 0) + (m[:decode_time] || 0)
    observe(:presence_db_query_seconds, [], seconds(query))
    if q = m[:queue_time], do: observe(:presence_db_queue_seconds, [], seconds(q))
  end

  def handle_event([:phoenix, :endpoint, :stop], m, %{conn: conn}, _) do
    inc(:presence_http_requests_total, status: "#{div(conn.status || 0, 100)}xx")
    observe(:presence_http_request_seconds, [], seconds(m.duration))
  end

  def handle_event(_, _, _, _), do: :ok

  defp seconds(native), do: System.convert_time_unit(native, :native, :microsecond) / 1_000_000

  ## The text format

  @doc "Every metric, in Prometheus' text format (version 0.0.4)."
  def render do
    rows = :ets.tab2list(@table)

    kept =
      for {name, {type, help, o}} <- @defs do
        series =
          for row <- rows, match?({^name, _}, elem(row, 0)), do: {elem(elem(row, 0), 1), row}

        [header(name, type, help), Enum.map(Enum.sort(series), &kept(name, type, o, &1))]
      end

    [kept, Enum.map(gauges(), &gauge/1)]
  end

  defp header(name, type, help),
    do: [
      "# HELP ",
      to_string(name),
      " ",
      help,
      "\n# TYPE ",
      to_string(name),
      " ",
      to_string(type),
      "\n"
    ]

  defp kept(name, :counter, o, {labels, {_, n}}) do
    value = if s = o[:scale], do: n / s, else: n
    [to_string(name), labels(labels), " ", number(value), "\n"]
  end

  defp kept(name, :histogram, o, {labels, row}) do
    [_k, count, sum | counts] = Tuple.to_list(row)
    bounds = Enum.map(o[:buckets], &number/1) ++ ["+Inf"]

    {lines, _} =
      Enum.zip(bounds, counts)
      |> Enum.map_reduce(0, fn {le, c}, acc ->
        acc = acc + c
        {["#{name}_bucket", labels(labels ++ [le: le]), " ", number(acc), "\n"], acc}
      end)

    [
      lines,
      ["#{name}_sum", labels(labels), " ", number(sum / 1_000_000), "\n"],
      ["#{name}_count", labels(labels), " ", number(count), "\n"]
    ]
  end

  # {name, type, help, [{labels, value}]}, read now.
  defp gauges do
    rooms = safe(&Rooms.stats/0, %{connections: 0, rooms: 0, members: 0, calls: 0, in_calls: 0})
    mem = :erlang.memory()
    {uptime_ms, _} = :erlang.statistics(:wall_clock)
    {reductions, _} = :erlang.statistics(:reductions)

    [
      {:presence_connections, :gauge, "This node's room connections (sum across nodes).",
       [{[], rooms.connections}]},
      {:presence_rooms, :gauge, "Rooms with anyone in them, cluster-wide (max across nodes).",
       [{[], rooms.rooms}]},
      {:presence_members, :gauge, "Members in rooms, cluster-wide (max across nodes).",
       [{[], rooms.members}]},
      {:presence_calls_active, :gauge, "Calls in progress, cluster-wide (max across nodes).",
       [{[], rooms.calls}]},
      {:presence_call_participants, :gauge,
       "Connections in calls, cluster-wide (max across nodes).", [{[], rooms.in_calls}]},
      {:presence_cluster_nodes, :gauge, "Nodes this one sees, itself included.",
       [{[], length(Node.list()) + 1}]},
      {:presence_usage_unflushed_seconds, :gauge,
       "Seconds since a usage flush last left nothing to retry.",
       [{[], usage_unflushed_seconds()}]},
      {:presence_usage_kept, :gauge, "App-days the last usage flush kept for the next try.",
       [{[], lookup(:usage_kept, 0)}]}
    ] ++
      pool() ++
      [
        {:presence_beam_memory_bytes, :gauge, "BEAM memory, by kind.",
         for(k <- [:total, :processes, :binary, :ets, :atom, :code], do: {[kind: k], mem[k]})},
        {:presence_beam_processes, :gauge, "Erlang processes.",
         [{[], :erlang.system_info(:process_count)}]},
        {:presence_beam_ports, :gauge, "Erlang ports (sockets, files).",
         [{[], :erlang.system_info(:port_count)}]},
        {:presence_beam_atoms, :gauge, "Atoms.", [{[], :erlang.system_info(:atom_count)}]},
        {:presence_beam_run_queue, :gauge, "Processes waiting to run, all schedulers.",
         [{[], :erlang.statistics(:total_run_queue_lengths)}]},
        {:presence_beam_schedulers, :gauge, "Schedulers online.",
         [{[], :erlang.system_info(:schedulers_online)}]},
        {:presence_beam_scheduler_utilization, :gauge,
         "Schedulers' busy share since the last scrape, 0 to 1 (CPU the BEAM really used).",
         [{[], utilization()}]},
        {:presence_beam_reductions_total, :counter, "Reductions (work done).",
         [{[], reductions}]},
        {:presence_beam_uptime_seconds, :gauge, "Seconds since the node started.",
         [{[], div(uptime_ms, 1000)}]}
      ]
  end

  # The normal schedulers' busy time over their wall time since the last
  # scrape (`:scheduler_wall_time`, turned on in init/1): unlike the OS's
  # CPU figure, it leaves out schedulers spinning while they wait for work.
  defp utilization do
    now = :erlang.statistics(:scheduler_wall_time) || []
    n = :erlang.system_info(:schedulers)
    now = for {id, active, total} <- now, id <= n, do: {id, active, total}
    before = lookup(:scheduler_wall_time, [])
    :ets.insert(@table, {:scheduler_wall_time, now})

    {active, total} =
      for {{id, a1, t1}, {id, a0, t0}} <- Enum.zip(Enum.sort(now), Enum.sort(before)),
          reduce: {0, 0},
          do: ({a, t} -> {a + a1 - a0, t + t1 - t0})

    if total > 0, do: Float.round(active / total, 4), else: 0.0
  end

  # The Repo's pool, where there's a database and its pool says.
  defp pool do
    with true <- Application.get_env(:presence_relay, :database, true),
         %{pid: pid} <- Ecto.Adapter.lookup_meta(PresenceRelay.Repo),
         [_ | _] = ms <- DBConnection.get_connection_metrics(pid) do
      size = PresenceRelay.Repo.config()[:pool_size] || 10

      [
        {:presence_db_pool_size, :gauge, "Postgres connections the pool may hold.", [{[], size}]},
        {:presence_db_pool_ready, :gauge, "Pooled Postgres connections idle.",
         [{[], ms |> Enum.map(& &1.ready_conn_count) |> Enum.sum()}]},
        {:presence_db_pool_queue, :gauge, "Callers waiting for a pooled connection.",
         [{[], ms |> Enum.map(& &1.checkout_queue_length) |> Enum.sum()}]}
      ]
    else
      _ -> []
    end
  rescue
    _ -> []
  catch
    :exit, _ -> []
  end

  defp gauge({name, type, help, series}) do
    [
      header(name, type, help),
      for({l, v} <- series, do: [to_string(name), labels(l), " ", number(v), "\n"])
    ]
  end

  defp lookup(key, default) do
    case :ets.lookup(@table, key) do
      [{_, v}] -> v
      [] -> default
    end
  end

  defp safe(fun, default) do
    fun.()
  rescue
    _ -> default
  catch
    :exit, _ -> default
  end

  defp labels([]), do: ""

  defp labels(labels) do
    inner = Enum.map_join(labels, ",", fn {k, v} -> ~s(#{k}="#{escape(to_string(v))}") end)
    "{" <> inner <> "}"
  end

  defp escape(v),
    do:
      v
      |> String.replace("\\", "\\\\")
      |> String.replace("\"", "\\\"")
      |> String.replace("\n", "\\n")

  defp number(n) when is_integer(n), do: Integer.to_string(n)
  defp number(n) when is_float(n), do: :erlang.float_to_binary(n, [:short])

  ## The table

  @impl true
  def init(_) do
    :ets.new(@table, [
      :named_table,
      :public,
      :set,
      write_concurrency: true,
      decentralized_counters: true
    ])

    :ets.insert(@table, {:usage_clean_at, System.monotonic_time(:millisecond)})
    :erlang.system_flag(:scheduler_wall_time, true)
    :telemetry.detach(__MODULE__)
    :ok = :telemetry.attach_many(__MODULE__, @events, &__MODULE__.handle_event/4, nil)
    {:ok, nil}
  end

  @impl true
  def terminate(_, _), do: :telemetry.detach(__MODULE__)
end
