defmodule PresenceRelay.UsageLoadTest do
  # Usage under a scripted load on two nodes (#318's P9): members over
  # three apps and three days (the clock injected), guests with a gid and
  # without, reconnects on the other node, a Usage restart mid-day, and
  # calls of known lengths, told on both nodes. The numbers the API gives
  # must equal the script's own count, exactly; and a capped app refuses
  # new people past its cap on either node.
  #
  # Both nodes write to Postgres for real (the peer has no sandbox), so
  # this runs outside the sandbox and deletes what it made.
  use ExUnit.Case, async: false
  import Phoenix.ConnTest
  import PresenceRelay.RelayCase

  alias PresenceRelay.{Apps, Calls, Cluster, Repo, Usage}
  alias PresenceRelay.Accounts.Account

  @endpoint PresenceRelayWeb.Endpoint
  @moduletag :cluster
  @origin "https://load.example"
  @full "this app is at its monthly limit"
  @d1 ~D[2026-09-30]
  @d2 ~D[2026-10-01]
  @d3 ~D[2026-10-15]

  setup_all do
    Ecto.Adapters.SQL.Sandbox.mode(Repo, :auto)
    {peer, node} = Cluster.start_peer!(database: true)

    on_exit(fn ->
      :peer.stop(peer)
      Ecto.Adapters.SQL.Sandbox.mode(Repo, :manual)
    end)

    %{peer: node}
  end

  setup %{peer: peer} do
    Usage.reset()
    :ok = :erpc.call(peer, Usage, :reset, [])
    n = System.unique_integer([:positive])
    owner = Repo.insert!(%Account{id: "acc_load#{n}", email: "load#{n}@example.com"})

    apps =
      for name <- ~w(a b c) do
        {:ok, app, _} = Apps.create(owner, %{"name" => name, "guests" => true})
        {:ok, _} = Apps.add_origin(app, @origin)
        app
      end

    on_exit(fn ->
      Application.delete_env(:presence_relay, :usage_today)
      # Their usage goes with them (on delete cascade).
      for a <- apps, do: Repo.delete!(a)
      Repo.delete!(owner)
    end)

    %{apps: apps, owner: owner}
  end

  test "MAU and call minutes over three apps, three days and two nodes, exactly", %{
    peer: peer,
    apps: [a, b, c],
    owner: owner
  } do
    here = node()

    # The script: {day, [{app, node, who}] joins, [{app, node, ms}] calls}.
    # `who`: {:member, sub}, {:gid, gid} (a guest with one) or :anon (a
    # guest without, new each socket).
    members = fn app, range -> for i <- range, do: {app, {:member, "u#{i}"}} end
    gids = fn app, range -> for i <- range, do: {app, {:gid, "guest-#{app.name}-#{i}0000"}} end
    anons = fn app, n -> for _ <- 1..n, do: {app, :anon} end

    # Everyone on a node by turn; the first few again on the other (a
    # reconnect, or a second tab).
    spread = fn people, again ->
      people
      |> Enum.with_index()
      |> Enum.flat_map(fn {{app, who}, i} ->
        n = if rem(i, 2) == 0, do: here, else: peer
        other = if n == here, do: peer, else: here
        first = {app, n, who}
        if i < again and who != :anon, do: [first, {app, other, who}], else: [first]
      end)
    end

    script = [
      {@d1,
       spread.(members.(a, 1..20) ++ gids.(a, 1..5) ++ anons.(a, 3), 8) ++
         spread.(members.(b, 1..6) ++ gids.(b, 1..2), 3),
       [{a, here, 61_000}, {a, here, 30_500}, {a, peer, 59_000}, {a, peer, 1}]},
      {@d2,
       spread.(members.(a, 15..30) ++ gids.(a, 4..6) ++ anons.(a, 2), 4) ++
         spread.(members.(b, 4..9), 2) ++ spread.(members.(c, 1..5) ++ gids.(c, 1..1), 6),
       [{a, peer, 600_000}, {a, here, 1}, {c, here, 59_999}, {c, peer, 60_001}]},
      {@d3, spread.(members.(a, 1..10) ++ gids.(a, 1..1), 11) ++ spread.(members.(b, 1..2), 0),
       [{b, peer, 119_999}, {b, here, 0}]}
    ]

    for {day, joins, calls} <- script do
      Cluster.put_env(peer, :usage_today, day)

      for {{app, n, who}, i} <- Enum.with_index(joins) do
        assert {:ok, _, _} = join(n, {day, i}, app, "#{day}-#{app.name}", who)

        # Midway through the first day the peer's Usage restarts (a deploy:
        # it flushes as it stops); the rest of the day carries on.
        if day == @d1 and i == div(length(joins), 2), do: restart(peer)
      end

      for {app, n, ms} <- calls do
        :ok = :erpc.call(n, Calls, :leave, [app.id, "room", "call", "u1", ms])
      end

      # Flushed some days, not others: a day's buffer may meet the next's.
      if day != @d2, do: flush(peer)
    end

    flush(peer)

    # The script's own count.
    {per_day, per_month} = expected(script)

    for app <- [a, b, c] do
      body =
        build_conn()
        |> init_test_session(account_id: owner.id)
        |> get("/api/v1/apps/#{app.id}/usage?months=2")
        |> json_response(200)

      want_months =
        for m <- [~D[2026-09-01], ~D[2026-10-01]] do
          {users, minutes} = Map.get(per_month, {app.id, m}, {0, 0})

          %{
            "month" => Calendar.strftime(m, "%Y-%m"),
            "active_users" => users,
            "call_minutes" => minutes
          }
        end

      assert body["months"] == want_months
      assert body["this_month"] == Map.delete(List.last(want_months), "month")

      for d <- body["days"] do
        {users, minutes} = Map.get(per_day, {app.id, Date.from_iso8601!(d["day"])}, {0, 0})
        assert {d["day"], d["active_users"], d["call_minutes"]} == {d["day"], users, minutes}
      end
    end

    # And by hand, so the script's count is checked too. App a, day 1: 20
    # members, 5 gid guests, 3 anonymous; 61 + 31 + 59 + 1 s = 3 min. Day 2:
    # u15–u30, gids 4–6, 2 anonymous; 600 + 1 s = 11 min. Day 3: u1–u10, gid
    # 1. October: u1–u10 and u15–u30, gids 1 and 4–6, 2 anonymous = 32.
    assert per_day[{a.id, @d1}] == {28, 3}
    assert per_day[{a.id, @d2}] == {21, 11}
    assert per_day[{a.id, @d3}] == {11, 0}
    assert per_month[{a.id, ~D[2026-09-01]}] == {28, 3}
    assert per_month[{a.id, ~D[2026-10-01]}] == {32, 11}
    # App b: 8, then u4–u9, then u1–u2 (October: u1, u2, u4–u9 = 8);
    # 119.999 s → 120 s → 2 min (a 0 ms leave is nothing).
    assert per_month[{b.id, ~D[2026-10-01]}] == {8, 2}
    # App c: 59.999 s → 60 s (1 min) and 60.001 s → 61 s: 121 s → 3 min a
    # day; minutes round per day, not per call.
    assert per_day[{c.id, @d2}] == {6, 3}
  end

  test "a capped app refuses new people on either node, and counted people get back in", %{
    peer: peer,
    apps: [app | _]
  } do
    here = node()
    Cluster.put_env(peer, :usage_today, @d3)
    {:ok, app} = Usage.put_cap(app, 4)
    r = "capped"

    # Two people on each node, then both flush: the month is at its cap.
    for {n, i} <- [{here, 1}, {peer, 2}, {here, 3}, {peer, 4}],
        do: assert({:ok, _, _} = join(n, i, app, r, {:member, "p#{i}"}))

    flush(peer)

    for n <- [here, peer],
        do: assert({:error, %{reason: @full}} = join(n, {:new, n}, app, r, {:member, "p5"}))

    # p1 joined here, p2 there: each gets back in on the other node too.
    assert {:ok, _, _} = join(peer, :p1_again, app, r, {:member, "p1"})
    assert {:ok, _, _} = join(here, :p2_again, app, r, {:member, "p2"})
    assert {:error, %{reason: @full}} = join(here, :guest, app, r, {:gid, "a-new-guest-1"})

    # A higher cap reaches both nodes at once.
    {:ok, app} = Usage.put_cap(app, 6)
    assert {:ok, _, _} = join(peer, :p5, app, r, {:member, "p5"})
    assert {:ok, _, _} = join(here, :g1, app, r, {:gid, "a-new-guest-1"})
    flush(peer)
    assert {:error, %{reason: @full}} = join(peer, :p6, app, r, {:member, "p6"})
    assert %{this_month: %{active_users: 6}} = Usage.report(app, 1, @d3)

    # The overshoot, bounded: neither node has seen the other's newest
    # people before they flush. At 6 of 8, each node lets two new people
    # in before a flush: 10, two over (the other node's unflushed ones).
    {:ok, app} = Usage.put_cap(app, 8)

    for {n, i} <- [{here, 7}, {peer, 8}, {here, 9}, {peer, 10}],
        do: assert({:ok, _, _} = join(n, {:over, i}, app, r, {:member, "p#{i}"}))

    flush(peer)
    assert %{this_month: %{active_users: 10}} = Usage.report(app, 1, @d3)

    for n <- [here, peer],
        do: assert({:error, %{reason: @full}} = join(n, {:late, n}, app, r, {:member, "p11"}))
  end

  # A client on node `n` joins `room` of `app` as `who`, and leaves again
  # (out of the sandbox, a channel process keeps the connection it used
  # until it exits: this keeps the test node's pool free).
  defp join(n, name, app, room, who) do
    case client_with(n, name, room, params(app, room, who), %{origin: @origin}) do
      {:ok, pid, reply} ->
        leave(pid, name)
        {:ok, pid, reply}

      refused ->
        refused
    end
  end

  defp params(app, room, {:member, sub}),
    do: %{"token" => token(%{"room" => room, "sub" => sub, "name" => sub}, app.id)}

  defp params(app, room, who) do
    p = %{"key" => key(app), "room" => room, "name" => "Guest"}
    with {:gid, gid} <- who, do: Map.put(p, "gid", gid), else: (_ -> p)
  end

  defp key(app), do: hd(app.keys).key

  defp flush(peer) do
    :ok = Usage.flush()
    :ok = :erpc.call(peer, Usage, :flush, [])
  end

  # Stops the peer's Usage (it flushes) and waits for its restart.
  defp restart(peer) do
    pid = :erpc.call(peer, Process, :whereis, [Usage])
    :ok = :erpc.call(peer, GenServer, :stop, [Usage])

    Enum.find_value(1..100, fn _ ->
      case :erpc.call(peer, Process, :whereis, [Usage]) do
        p when is_pid(p) and p != pid -> true
        _ -> Process.sleep(10) && nil
      end
    end) || flunk("the peer's Usage didn't restart")
  end

  # Per {app, day}: distinct people and call minutes; per {app, month}:
  # distinct people and the sum of its days' minutes. A gid-less guest is
  # someone new each time; each leave is rounded up to a second.
  defp expected(script) do
    keys =
      for {day, joins, _} <- script, {app, _n, who} <- joins do
        key =
          case who do
            {:member, sub} -> sub
            {:gid, gid} -> "gid#" <> gid
            :anon -> make_ref()
          end

        {app.id, day, key}
      end

    seconds =
      for {day, _, calls} <- script, {app, _, ms} <- calls, reduce: %{} do
        acc -> Map.update(acc, {app.id, day}, div(ms + 999, 1000), &(&1 + div(ms + 999, 1000)))
      end

    users = fn ks -> ks |> Enum.uniq() |> length() end

    day_keys =
      Enum.group_by(keys, fn {a, d, _} -> {a, d} end, fn {_, _, k} -> k end)

    per_day =
      for {ad, _} <- Map.merge(day_keys, seconds), into: %{} do
        {ad, {users.(Map.get(day_keys, ad, [])), Usage.minutes(Map.get(seconds, ad, 0))}}
      end

    month_keys =
      Enum.group_by(keys, fn {a, d, _} -> {a, Usage.month(d)} end, fn {_, _, k} -> k end)

    per_month =
      for {{a, d}, {_, minutes}} <- per_day, reduce: %{} do
        acc ->
          m = Usage.month(d)
          {u, mins} = Map.get(acc, {a, m}, {users.(Map.get(month_keys, {a, m}, [])), 0})
          Map.put(acc, {a, m}, {u, mins + minutes})
      end

    {per_day, per_month}
  end
end
