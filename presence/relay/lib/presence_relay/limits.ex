defmodule PresenceRelay.Limits do
  @moduledoc """
  Abuse limits (#318's P12): how often one client may do something that
  costs us, counted in fixed windows.

  Each limit is `{hits, seconds}` in config `:rate_limits` (nil turns it
  off), keyed by what's being limited: a client IP
  (`PresenceRelayWeb.ClientIP`), or for email a hash of the address.

  | limit | key | default |
  |---|---|---|
  | `socket_connect` | IP | 120 a minute |
  | `email_ip` | IP | 10 an hour |
  | `email_address` | address | 5 an hour |
  | `meet_room` | IP | 20 an hour |
  | `api_write` | IP | 120 a minute |
  | `demo_token` | IP | 120 an hour |

  `RATE_LIMITS` (config/runtime.exs) changes any of them, as
  `name=hits/seconds,…` (`name=off` turns one off).

  ## Per node

  The counts are this node's (an ETS counter per key and window, bumped
  atomically, so a hit costs no message and no lock). A client whose
  requests land on several nodes gets up to the limit on each: with N
  machines, at most N times the number. That's still a ceiling an abuser
  hits at once, which is the point; a cluster-wide count would cost a
  round trip per hit. Fixed windows let a client spend a window's hits at
  its end and the next window's at the start: at most twice the number in
  one window's time.

  A concurrent-connection cap per app is the other kind of limit, and
  cluster-wide: `PresenceRelay.Rooms` counts each app's connections in a
  `:pg` group.
  """
  use GenServer

  @table :presence_relay_limits
  @sweep_ms 60_000

  def start_link(_), do: GenServer.start_link(__MODULE__, nil, name: __MODULE__)

  @doc "The limit's `{hits, seconds}` (nil: off)."
  def limit(name), do: Map.get(Application.get_env(:presence_relay, :rate_limits, %{}), name)

  @doc """
  One hit on limit `name` for `key`: `:ok`, or `{:error, retry_after}` in
  whole seconds once the window's hits are spent. A nil key (no client IP)
  isn't limited.
  """
  def hit(_name, nil), do: :ok

  def hit(name, key) do
    case limit(name) do
      {hits, seconds} when is_integer(hits) and is_integer(seconds) and seconds > 0 ->
        count(name, key, hits, seconds * 1000)

      _ ->
        :ok
    end
  end

  defp count(name, key, hits, window_ms) do
    now = System.monotonic_time(:millisecond)
    window = Integer.floor_div(now, window_ms)
    ends = (window + 1) * window_ms
    k = {name, key, window}

    n = :ets.update_counter(@table, k, {2, 1}, {k, 0, ends})

    if n <= hits do
      :ok
    else
      :telemetry.execute([:presence_relay, :limit, :refused], %{count: 1}, %{limit: name})
      {:error, max(1, div(ends - now + 999, 1000))}
    end
  rescue
    # No table (this node's Limits is restarting): let it through.
    ArgumentError -> :ok
  end

  @doc """
  Applies `RATE_LIMITS` (config `:rate_limits_spec`, from runtime.exs)
  over the configured limits, at boot; raises on one it can't read.
  """
  def configure! do
    case Application.get_env(:presence_relay, :rate_limits_spec) do
      spec when is_binary(spec) ->
        base = Application.get_env(:presence_relay, :rate_limits, %{})
        Application.put_env(:presence_relay, :rate_limits, parse(spec, base))

      _ ->
        :ok
    end
  end

  @doc false
  # Tests: forget every count.
  def reset, do: :ets.delete_all_objects(@table)

  @doc """
  `RATE_LIMITS`' form, `name=hits/seconds,…` (`name=off`), over `base`:
  the limits it names changed, the rest kept. Raises on a name or number
  it doesn't know.
  """
  def parse(spec, base) do
    spec
    |> String.split(",", trim: true)
    |> Enum.reduce(base, fn pair, acc ->
      with [name, value] <- String.split(String.trim(pair), "=", parts: 2),
           name when is_atom(name) and not is_nil(name) <- known(base, name),
           {:ok, v} <- value(value) do
        Map.put(acc, name, v)
      else
        _ -> raise "RATE_LIMITS: #{inspect(pair)} isn't name=hits/seconds or name=off"
      end
    end)
  end

  defp known(base, name), do: Enum.find(Map.keys(base), &(Atom.to_string(&1) == name))

  defp value("off"), do: {:ok, nil}

  defp value(v) do
    with [h, s] <- String.split(v, "/"),
         {h, ""} when h >= 0 <- Integer.parse(h),
         {s, ""} when s > 0 <- Integer.parse(s),
         do: {:ok, {h, s}},
         else: (_ -> :error)
  end

  @impl true
  def init(_) do
    :ets.new(@table, [
      :named_table,
      :public,
      :set,
      write_concurrency: true,
      decentralized_counters: true
    ])

    Process.send_after(self(), :sweep, @sweep_ms)
    {:ok, nil}
  end

  # Windows that have ended are forgotten.
  @impl true
  def handle_info(:sweep, s) do
    now = System.monotonic_time(:millisecond)
    :ets.select_delete(@table, [{{:_, :_, :"$1"}, [{:<, :"$1", now}], [true]}])
    Process.send_after(self(), :sweep, @sweep_ms)
    {:noreply, s}
  end
end
