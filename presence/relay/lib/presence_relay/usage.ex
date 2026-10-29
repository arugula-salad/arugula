defmodule PresenceRelay.Usage do
  @moduledoc """
  Usage metering and caps (#318's P9): each app's monthly active users
  and call minutes, rolled up by day in Postgres, and an optional cap on
  monthly active users, checked at join.

  ## What's metered

  Apps in the database only (`meet` and `app_…`): static apps
  (`PRESENCE_APPS`) and the demo's are neither metered nor capped, and
  a relay without a database meters nothing.

  ## Monthly active users

  The distinct people who joined a room of the app in a calendar month
  (UTC), each by a **metering key**:

  - a member by room token or by provider token: their `sub`;
  - a guest (publishable key and a name): `gid#<gid>` when the socket
    sent a sound `gid` (`[A-Za-z0-9_-]{8,64}`, a random id the browser
    made once and keeps), else their `sub`, which is new for every
    socket, so a guest without one counts again on each connection.

  `gid` is unverified and for metering only: it never names anyone in a
  room, and a page that lies about it only miscounts itself.

  Each person is a row of `usage_members` (app, month, key), so the
  month's count is exact however many nodes saw them; its `days` has bit
  d-1 set for each day d they joined on, and each day's `active_users`
  in `usage_days` goes up by the rows whose bit was new, so a person
  counts once a day too.

  ## Call minutes

  From `PresenceRelay.Calls`' `:leave` telemetry: each person's time in
  each call, rounded up to a whole second, summed per app and day (the
  day the person left: a call over midnight counts on the day it ends)
  into `usage_days.call_seconds`. A day's minutes are its seconds over 60
  rounded up; a month's are the sum of its days' minutes.

  ## Batched, per node

  Joins and leaves are buffered in this process (a set of keys per app
  and day, seconds per app and day) and written every
  `usage_flush_ms` (5 s), one transaction per app and day: insert the
  members (`ON CONFLICT` sets the day's bit, if new), then add the new
  ones and the seconds to the day. A flush that fails is kept and tried
  again; one for an app that's gone is dropped. Stopping (a deploy)
  flushes; a crash loses at most one interval's buffer (a few seconds of
  joins, which the next join puts back, and of call time, which is lost).

  ## Caps

  An app's cap (nil, none): the owner's `mau_cap`, or its plan's if
  that's lower (#318's P11, `PresenceRelay.Billing.cap/1`). Once the
  month's count reaches it, a person not yet counted this month is
  refused at join with "this app is at its monthly limit"; anyone
  already counted gets in.

  Each node caches every app's cap, and for a capped app its month's
  count (reloaded every `usage_cache_ms`, 30 s, and recounted after a
  flush writes the app, when the new count goes to every node), the
  people it has seen this month, and how many new ones it has let in
  since its last count. A join costs no query while the count plus those
  is under the cap. At the cap, this node flushes the app and asks
  Postgres whether this person is already counted and what the count is
  now, and decides on that. So one node never overshoots; across nodes
  the overshoot is at most the new people the other nodes let in and
  haven't flushed yet (about one flush interval's worth each), plus
  joins racing the same check. A refused stranger costs a query each
  try.
  """
  use GenServer
  require Logger
  import Ecto.Query

  alias PresenceRelay.{Apps, Billing, Repo}
  alias PresenceRelay.Apps.App
  alias PresenceRelay.Usage.{Day, Member}

  # app => {cap, month, count, loaded at}: capped and uncapped apps.
  @caps __MODULE__.Caps
  # {app, month, key}: people known to count this month (capped apps only).
  @seen __MODULE__.Seen
  # {app, month} => new people let in here since the last count.
  @extra __MODULE__.Extra
  @full "this app is at its monthly limit"
  @gid ~r/\A[A-Za-z0-9_-]{8,64}\z/
  @chunk 5_000
  @max_cap 2_147_483_647

  def start_link(_), do: GenServer.start_link(__MODULE__, nil, name: __MODULE__)

  @doc "Today (UTC), or config `:usage_today` (a `Date`) in tests."
  def today do
    case Application.get_env(:presence_relay, :usage_today) do
      %Date{} = d -> d
      _ -> Date.utc_today()
    end
  end

  @doc "The month a day is in, as its first day."
  def month(%Date{} = d), do: Date.beginning_of_month(d)

  @doc "Whether `app` is metered (and may be capped): an app in the database."
  def metered?(app), do: Apps.database?() and Apps.db_id?(app) and not Apps.static?(app)

  @doc "A guest's metering key: `gid#<gid>` for a sound `gid` param, else its `sub`."
  def guest_key(sub, %{"gid" => gid}) when is_binary(gid),
    do: if(gid =~ @gid, do: "gid#" <> gid, else: sub)

  def guest_key(sub, _), do: sub

  @doc """
  Whether the person metered as `key` may join a room of `app` this
  month: `:ok`, or `{:error, "this app is at its monthly limit"}`.
  """
  def admit(app, key) do
    if metered?(app), do: check(app, key, month(today())), else: :ok
  rescue
    # No tables (this node's Usage is restarting) or no database: let
    # them in rather than refuse on our account.
    e in [ArgumentError, DBConnection.ConnectionError, Postgrex.Error] ->
      Logger.warning("usage: admitting without a check: #{Exception.message(e)}")
      :ok
  catch
    # The at-cap check didn't answer (Usage down, or Postgres slow).
    :exit, why ->
      Logger.warning("usage: admitting without a check: #{inspect(why)}")
      :ok
  end

  @doc "The person metered as `key` joined a room of `app`: counted at the next flush."
  def joined(app, key) do
    if metered?(app) do
      day = today()
      m = month(day)

      # A capped app's new person counts against the cap here at once.
      if capped?(app, m) and :ets.insert_new(@seen, {{app, m, key}}),
        do: :ets.update_counter(@extra, {app, m}, 1, {{app, m}, 0})

      GenServer.cast(__MODULE__, {:joined, app, day, key})
    end

    :ok
  rescue
    ArgumentError -> :ok
  end

  @doc "Writes this node's buffer now (tests; a flush also runs every interval)."
  def flush, do: GenServer.call(__MODULE__, :flush, 30_000)

  @doc false
  # Tests: drops this node's buffer and caches.
  def reset, do: GenServer.call(__MODULE__, :reset)

  @doc "The app's cap (nil for none)."
  def cap(%App{mau_cap: cap}), do: cap

  @doc """
  Sets `app`'s cap on monthly active users: a whole number, or nil for
  none. Every node forgets its cached cap.
  """
  def put_cap(%App{} = app, cap)
      when is_nil(cap) or (is_integer(cap) and cap >= 0 and cap <= @max_cap) do
    with {:ok, app} <- app |> Ecto.Changeset.change(mau_cap: cap) |> Repo.update() do
      forget_everywhere(app.id)
      {:ok, app}
    end
  end

  def put_cap(_, _), do: {:error, "a cap is a whole number of active users, or null"}

  @doc """
  Every node forgets `app`'s cached cap: its cap changed (the owner's,
  or its plan's: `PresenceRelay.Billing`).
  """
  def forget_everywhere(app) do
    forget(app)
    :erpc.multicast(Node.list(), __MODULE__, :forget, [app])
    :ok
  end

  @doc false
  # Another node's put_cap: drop this node's cached cap and count.
  def forget(app) do
    if :ets.whereis(@caps) != :undefined, do: :ets.delete(@caps, app)
    :ok
  end

  @doc false
  # Another node's counts of capped apps' months, just after it flushed:
  # a count only goes up within a month.
  def counted(counts) do
    for {app, m, n} <- counts do
      case :ets.lookup(@caps, app) do
        [{^app, cap, ^m, count, at}] when n > count -> :ets.insert(@caps, {app, cap, m, n, at})
        _ -> :ok
      end
    end

    :ok
  rescue
    ArgumentError -> :ok
  end

  @doc """
  The app's usage as the API shows it (`GET /api/v1/apps/:id/usage`):
  the last `months` months and 31 days (oldest first), its cap, and this
  month. Up to a flush interval behind.
  """
  def report(%App{id: app} = a, months \\ 3, today \\ today()) do
    this = month(today)
    firsts = for i <- (months - 1)..0//-1, do: Date.shift(this, month: -i)
    from_day = Enum.min([hd(firsts), Date.add(today, -30)], Date)

    mau =
      from(u in Member,
        where: u.app_id == ^app and u.month >= ^hd(firsts) and u.month <= ^this,
        group_by: u.month,
        select: {u.month, count()}
      )
      |> Repo.all()
      |> Map.new()

    days =
      from(d in Day,
        where: d.app_id == ^app and d.day >= ^from_day and d.day <= ^today,
        select: {d.day, {d.active_users, d.call_seconds}}
      )
      |> Repo.all()
      |> Map.new()

    month_view = fn first ->
      minutes =
        for {d, {_, secs}} <- days, month(d) == first, reduce: 0, do: (n -> n + minutes(secs))

      %{
        month: Calendar.strftime(first, "%Y-%m"),
        active_users: Map.get(mau, first, 0),
        call_minutes: minutes
      }
    end

    months = Enum.map(firsts, month_view)

    %{
      months: months,
      days:
        for i <- 30..0//-1 do
          d = Date.add(today, -i)
          {users, secs} = Map.get(days, d, {0, 0})
          %{day: Date.to_iso8601(d), active_users: users, call_minutes: minutes(secs)}
        end,
      # The owner's own cap, and the one enforced (the lower of it and
      # the plan's: PresenceRelay.Billing).
      cap: %{active_users: a.mau_cap, limit: Billing.cap(app)},
      this_month: Map.delete(List.last(months), :month)
    }
  end

  @doc "Seconds in calls as billed minutes: rounded up."
  def minutes(seconds), do: div(seconds + 59, 60)

  @doc false
  # Calls' telemetry: each person's time in a call, in whole seconds
  # (rounded up), on today's date.
  def handle_event([:presence_relay, :call, :leave], %{duration_ms: ms}, %{app: app}, _)
      when is_integer(ms) and ms > 0 do
    if metered?(app), do: GenServer.cast(__MODULE__, {:call, app, today(), div(ms + 999, 1000)})
    :ok
  end

  def handle_event(_, _, _, _), do: :ok

  ## Caps, at join

  defp check(app, key, m) do
    case cached(app, m) do
      {nil, _} ->
        :ok

      {cap, count} ->
        cond do
          :ets.member(@seen, {app, m, key}) -> :ok
          count + extra(app, m) < cap -> :ok
          true -> GenServer.call(__MODULE__, {:check, app, m, key}, 10_000)
        end
    end
  end

  # The app's cap and month count, cached for `usage_cache_ms`.
  defp cached(app, m) do
    now = System.monotonic_time(:millisecond)
    fresh = now - cache_ms()

    case :ets.lookup(@caps, app) do
      [{^app, cap, ^m, count, at}] when at > fresh -> {cap, count}
      _ -> load(app, m, now)
    end
  end

  # The cap at join is the plan's and the owner's (PresenceRelay.Billing).
  defp load(app, m, now) do
    cap = Billing.cap(app)
    count = if cap, do: count(app, m), else: 0
    :ets.insert(@caps, {app, cap, m, count, now})
    {cap, count}
  end

  defp capped?(app, m),
    do: match?([{_, cap, ^m, _, _}] when is_integer(cap), :ets.lookup(@caps, app))

  defp extra(app, m) do
    case :ets.lookup(@extra, {app, m}) do
      [{_, n}] -> n
      [] -> 0
    end
  end

  defp count(app, m),
    do: Repo.aggregate(from(u in Member, where: u.app_id == ^app and u.month == ^m), :count)

  defp counted?(app, m, key),
    do:
      Repo.exists?(
        from(u in Member, where: u.app_id == ^app and u.month == ^m and u.member_key == ^key)
      )

  defp cache_ms, do: Application.get_env(:presence_relay, :usage_cache_ms, 30_000)
  defp flush_ms, do: Application.get_env(:presence_relay, :usage_flush_ms, 5_000)

  ## The buffer

  @impl true
  def init(_) do
    # Stopping (a deploy) flushes first: see terminate/2.
    Process.flag(:trap_exit, true)
    opts = [:named_table, :public, :set, read_concurrency: true, write_concurrency: true]
    for t <- [@caps, @seen, @extra], do: :ets.new(t, opts)

    :telemetry.detach(__MODULE__)

    :ok =
      :telemetry.attach(
        __MODULE__,
        [:presence_relay, :call, :leave],
        &__MODULE__.handle_event/4,
        nil
      )

    schedule()
    {:ok, empty()}
  end

  @impl true
  def handle_cast({:joined, app, day, key}, s) do
    {:noreply,
     %{s | members: Map.update(s.members, {app, day}, MapSet.new([key]), &MapSet.put(&1, key))}}
  end

  def handle_cast({:call, app, day, seconds}, s),
    do: {:noreply, %{s | calls: Map.update(s.calls, {app, day}, seconds, &(&1 + seconds))}}

  @impl true
  def handle_call(:flush, _from, s), do: {:reply, :ok, flush(s, fn _ -> true end)}

  def handle_call(:reset, _from, _s) do
    for t <- [@caps, @seen, @extra], do: :ets.delete_all_objects(t)
    {:reply, :ok, empty()}
  end

  # A join at the cap: this node's buffer for the app first, then the
  # database decides, on a fresh cap and count.
  def handle_call({:check, app, m, key}, _from, s) do
    s = flush(s, fn {a, _} -> a == app end)

    reply =
      if :ets.member(@seen, {app, m, key}) or counted?(app, m, key) do
        :ets.insert(@seen, {{app, m, key}})
        :ok
      else
        case load(app, m, System.monotonic_time(:millisecond)) do
          {cap, n} when is_integer(cap) and n >= cap -> {:error, @full}
          _ -> :ok
        end
      end

    {:reply, reply, s}
  rescue
    e in [DBConnection.ConnectionError, Postgrex.Error] ->
      Logger.warning("usage: admitting without a check: #{Exception.message(e)}")
      {:reply, :ok, s}
  end

  @impl true
  def handle_info(:flush, s) do
    s = flush(s, fn _ -> true end)
    schedule()
    {:noreply, s}
  end

  def handle_info(_, s), do: {:noreply, s}

  @impl true
  def terminate(_reason, s) do
    flush(s, fn _ -> true end)
    :ok
  end

  defp empty, do: %{members: %{}, calls: %{}}

  defp schedule do
    case flush_ms() do
      ms when is_integer(ms) -> Process.send_after(self(), :flush, ms)
      # `:manual` (tests): only flush/0.
      _ -> nil
    end
  end

  # Writes the buffer's {app, day}s that `pick?` picks, keeping any that
  # failed for the next flush; then recounts the capped apps written.
  defp flush(s, pick?) do
    if Apps.database?(), do: do_flush(s, pick?), else: empty()
  end

  defp do_flush(s, pick?) do
    taken = Map.new(:ets.tab2list(@extra))
    picked = (Map.keys(s.members) ++ Map.keys(s.calls)) |> Enum.uniq() |> Enum.filter(pick?)

    {s, written} =
      Enum.reduce(picked, {s, MapSet.new()}, fn {app, day} = k, {s, written} ->
        members = Map.get(s.members, k, MapSet.new())
        seconds = Map.get(s.calls, k, 0)

        case write(app, day, members, seconds) do
          :retry ->
            {s, written}

          :ok ->
            {%{s | members: Map.delete(s.members, k), calls: Map.delete(s.calls, k)},
             MapSet.put(written, app)}
        end
      end)

    recount(written, taken)
    s
  end

  # The capped apps just written: their month's count from Postgres, and
  # the new people let in here before the flush no longer extra.
  defp recount(apps, taken) do
    this = month(today())

    counts =
      for app <- apps, capped?(app, this) do
        [{_, cap, _, _, at}] = :ets.lookup(@caps, app)
        n = count(app, this)
        :ets.insert(@caps, {app, cap, this, n, at})
        taken = Map.get(taken, {app, this}, 0)
        if taken > 0, do: :ets.update_counter(@extra, {app, this}, -taken, {{app, this}, 0})
        {app, this, n}
      end

    # The other nodes hear the new counts now, not when their caches
    # next expire.
    if counts != [], do: :erpc.multicall(Node.list(), __MODULE__, :counted, [counts], 1_000)

    # Last month's people and counts are done with.
    :ets.select_delete(@seen, [{{{:_, :"$1", :_}}, [{:"=/=", :"$1", {:const, this}}], [true]}])
    :ets.select_delete(@extra, [{{{:_, :"$1"}, :_}, [{:"=/=", :"$1", {:const, this}}], [true]}])
    :ok
  rescue
    e in [DBConnection.ConnectionError, Postgrex.Error] ->
      Logger.warning("usage: couldn't recount: #{Exception.message(e)}")
      :ok
  end

  # One app's day: its people (setting the day's bit; the rows changed are
  # the people new today) and seconds in calls. `:retry` when Postgres
  # couldn't take it; dropped when the app is gone.
  defp write(app, day, members, seconds) do
    if MapSet.size(members) > 0 or seconds > 0, do: write!(app, day, members, seconds)
    :ok
  rescue
    e in Postgrex.Error ->
      if match?(%{postgres: %{code: :foreign_key_violation}}, e) do
        Logger.warning("usage: dropped #{app} #{day}: no such app")
        :ok
      else
        Logger.warning("usage: #{app} #{day} kept for the next flush: #{Exception.message(e)}")
        :retry
      end

    e in DBConnection.ConnectionError ->
      Logger.warning("usage: #{app} #{day} kept for the next flush: #{Exception.message(e)}")
      :retry
  end

  defp write!(app, day, members, seconds) do
    m = month(day)
    bit = Bitwise.bsl(1, day.day - 1)

    Repo.transaction(fn ->
      new =
        members
        |> Enum.map(&%{app_id: app, month: m, member_key: &1, days: bit})
        |> Enum.chunk_every(@chunk)
        |> Enum.reduce(0, fn rows, n ->
          {k, _} =
            Repo.insert_all(Member, rows,
              on_conflict: new_day(),
              conflict_target: [:app_id, :month, :member_key]
            )

          n + k
        end)

      Repo.insert_all(
        Day,
        [%{app_id: app, day: day, active_users: new, call_seconds: seconds}],
        on_conflict: [inc: [active_users: new, call_seconds: seconds]],
        conflict_target: [:app_id, :day]
      )
    end)
  end

  # A person already counted this month: set today's bit, if it isn't.
  defp new_day do
    from(u in Member,
      update: [set: [days: fragment("? | EXCLUDED.days", u.days)]],
      where: fragment("? & EXCLUDED.days = 0", u.days)
    )
  end
end
