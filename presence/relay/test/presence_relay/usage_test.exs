defmodule PresenceRelay.UsageTest do
  # Usage metering and caps (#318's P9) on one node: who counts, once a
  # month and once a day; call seconds and minutes; the cap at join; and
  # the owner's API. The two-node load is usage_load_test.exs.
  use PresenceRelay.RelayCase, async: false
  import Plug.Conn
  import Phoenix.ConnTest
  import Ecto.Query

  alias PresenceRelay.{Apps, Calls, Repo, Usage}
  alias PresenceRelay.Accounts.Account
  alias PresenceRelay.Usage.{Day, Member}
  alias PresenceRelayWeb.Socket

  @origin "https://acme.com"
  @full "this app is at its monthly limit"

  setup do
    Usage.reset()
    on_exit(fn -> Application.delete_env(:presence_relay, :usage_today) end)
    n = System.unique_integer([:positive])
    owner = Repo.insert!(%Account{id: "acc_u#{n}", email: "u#{n}@example.com"})
    {:ok, app, _} = Apps.create(owner, %{"name" => "Acme", "guests" => true})
    {:ok, _} = Apps.add_origin(app, @origin)
    [key] = app.keys
    %{app: app, key: key.key, owner: owner}
  end

  defp on(day), do: Application.put_env(:presence_relay, :usage_today, day)

  defp member(app, name, room, sub),
    do:
      client_with(node(), name, room, %{
        "token" => token(%{"room" => room, "sub" => sub, "name" => "M"}, app.id)
      })

  defp guest(key, name, room, gid \\ nil) do
    params = %{"key" => key, "room" => room, "name" => "Guest"}
    params = if gid, do: Map.put(params, "gid", gid), else: params
    client_with(node(), name, room, params, %{origin: @origin})
  end

  defp days(app),
    do:
      Repo.all(
        from(d in Day,
          where: d.app_id == ^app.id,
          order_by: d.day,
          select: {d.day, d.active_users, d.call_seconds}
        )
      )

  defp mau(app, month),
    do:
      Repo.aggregate(from(u in Member, where: u.app_id == ^app.id and u.month == ^month), :count)

  test "a guest meters by a sound gid, else by its own sub; members by their sub", %{key: key} do
    p = %{"key" => key, "room" => "lobby", "name" => "Ada"}
    {:ok, a} = Socket.admit(Map.put(p, "gid", "abcDEF123_-x"), @origin)
    assert a.meter == "gid#abcDEF123_-x"
    assert a.sub =~ ~r/\Ag_/
    {:ok, b} = Socket.admit(Map.put(p, "gid", "short"), @origin)
    assert b.meter == b.sub
    {:ok, c} = Socket.admit(p, @origin)
    assert c.meter == c.sub
    {:ok, t} = Socket.admit(%{"token" => token(%{"sub" => "u1"})}, nil)
    assert t.meter == "u1"
  end

  test "people count once a month and once a day; guests by gid; reconnects add no one", %{
    app: app,
    key: key
  } do
    r = room()
    on(~D[2026-09-30])
    {:ok, a, _} = member(app, :a, r, "alice")
    {:ok, _, _} = member(app, :b, r, "bob")
    leave(a, :a)
    # Alice again (a reconnect), a guest with a gid twice, one without twice.
    {:ok, _, _} = member(app, :a2, r, "alice")
    {:ok, _, %{you: g1}} = guest(key, :g1, r, "guest-one-1234")
    {:ok, _, %{you: g2}} = guest(key, :g2, r, "guest-one-1234")
    assert g1 != g2
    {:ok, _, _} = guest(key, :n1, r)
    {:ok, _, _} = guest(key, :n2, r)
    :ok = Usage.flush()

    # Alice and Bob, the gid guest once, and each gid-less socket.
    assert days(app) == [{~D[2026-09-30], 5, 0}]
    assert mau(app, ~D[2026-09-01]) == 5

    # Next day, next month: Alice again and someone new; the day after,
    # Alice once more (a new day, not a new person this month).
    on(~D[2026-10-01])
    {:ok, _, _} = member(app, :a3, r, "alice")
    {:ok, _, _} = guest(key, :g3, r, "guest-one-1234")
    {:ok, _, _} = member(app, :c, r, "carol")
    :ok = Usage.flush()
    on(~D[2026-10-02])
    {:ok, _, _} = member(app, :a4, r, "alice")
    :ok = Usage.flush()
    # A flush that's already been written adds nothing again.
    {:ok, _, _} = member(app, :a5, r, "alice")
    :ok = Usage.flush()

    assert days(app) == [
             {~D[2026-09-30], 5, 0},
             {~D[2026-10-01], 3, 0},
             {~D[2026-10-02], 1, 0}
           ]

    assert mau(app, ~D[2026-10-01]) == 3
  end

  test "call time: each leave rounded up to a second, a day's seconds up to minutes", %{app: app} do
    r = room()
    on(~D[2026-10-29])
    # A real call: rung, answered, left. Each side was in it under a second.
    {:ok, a, _} = member(app, :a, r, "alice")
    {:ok, b, _} = member(app, :b, r, "bob")
    send_as(a, "call", %{"to" => "bob"})
    assert_receive {:replied, :a, :ok, %{id: id}}, 1_000
    assert_receive {:pushed, :b, "call", %{id: ^id}}, 1_000
    send_as(b, "call_answer", %{"id" => id, "yes" => true})
    assert_receive {:pushed, :a, "call_join", _}, 1_000
    assert_receive {:pushed, :b, "call_join", _}, 1_000
    Process.sleep(20)
    send_as(a, "call_leave", %{"id" => id})
    send_as(b, "call_leave", %{"id" => id})
    assert_receive {:replied, :a, :ok, _}, 1_000
    assert_receive {:replied, :b, :ok, _}, 1_000
    :ok = Usage.flush()
    assert [{_, 2, 2}] = days(app)

    # Known lengths, as Calls tells them: 61 s, 30.5 s (31), 1 ms (1).
    for ms <- [61_000, 30_500, 1] do
      Calls.leave(app.id, r, "c1", "alice", ms)
    end

    # A static app's calls aren't metered.
    Calls.leave("acme", r, "c2", "alice", 600_000)
    :ok = Usage.flush()
    assert [{_, 2, 95}] = days(app)
    assert %{this_month: %{active_users: 2, call_minutes: 2}} = Usage.report(Repo.reload(app))
  end

  test "static and demo apps aren't metered" do
    r = room()
    {:ok, _, _} = client(:a, r, "alice")
    :ok = Usage.flush()
    assert Repo.aggregate(Member, :count) == 0
    refute Usage.metered?("acme")
    refute Usage.metered?("demo")
  end

  test "a capped app refuses new people at the cap and lets counted ones back", %{app: app} do
    r = room()
    on(~D[2026-10-29])
    {:ok, app} = Usage.put_cap(app, 2)
    {:ok, a, _} = member(app, :a, r, "alice")
    # Not flushed yet: this node still counts Bob against the cap.
    {:ok, _, _} = member(app, :b, r, "bob")
    assert {:error, %{reason: @full}} = member(app, :c, r, "carol")

    # Alice back: known here.
    leave(a, :a)
    {:ok, _, _} = member(app, :a2, r, "alice")

    # A node that never saw her (its caches empty) asks Postgres: counted.
    Usage.reset()
    {:ok, _, _} = member(app, :a3, r, "alice")
    assert {:error, %{reason: @full}} = member(app, :c2, r, "carol")
    assert mau(app, ~D[2026-10-01]) == 2

    # A higher cap lets one more in; no cap, anyone.
    {:ok, app} = Usage.put_cap(app, 3)
    {:ok, _, _} = member(app, :c3, r, "carol")
    assert {:error, %{reason: @full}} = member(app, :d, r, "dan")
    {:ok, _} = Usage.put_cap(app, nil)
    {:ok, _, _} = member(app, :d2, r, "dan")

    # A new month starts from nothing.
    {:ok, app} = Usage.put_cap(app, 1)
    on(~D[2026-11-01])
    {:ok, _, _} = member(app, :d3, r, "dan")
    assert {:error, %{reason: @full}} = member(app, :a4, r, "alice")
    :ok = Usage.flush()
    assert mau(app, ~D[2026-10-01]) == 4
    assert mau(app, ~D[2026-11-01]) == 1
  end

  defp as(%Account{id: id}), do: build_conn() |> init_test_session(account_id: id)

  defp put_json(conn, path, body),
    do:
      conn |> put_req_header("content-type", "application/json") |> put(path, Jason.encode!(body))

  test "the usage API: months and 31 days oldest first, the cap, this month", %{
    app: app,
    owner: owner
  } do
    on(~D[2026-10-29])
    r = room()
    {:ok, _, _} = member(app, :a, r, "alice")
    Calls.leave(app.id, r, "c1", "alice", 90_000)
    :ok = Usage.flush()

    body = as(owner) |> get("/api/v1/apps/#{app.id}/usage") |> json_response(200)

    assert body["months"] == [
             %{"month" => "2026-08", "active_users" => 0, "call_minutes" => 0},
             %{"month" => "2026-09", "active_users" => 0, "call_minutes" => 0},
             %{"month" => "2026-10", "active_users" => 1, "call_minutes" => 2}
           ]

    assert length(body["days"]) == 31
    assert hd(body["days"]) == %{"day" => "2026-09-29", "active_users" => 0, "call_minutes" => 0}

    assert List.last(body["days"]) == %{
             "day" => "2026-10-29",
             "active_users" => 1,
             "call_minutes" => 2
           }

    # With billing off, the cap enforced is the owner's (none here).
    assert body["cap"] == %{"active_users" => nil, "limit" => nil}
    assert body["this_month"] == %{"active_users" => 1, "call_minutes" => 2}

    body = as(owner) |> get("/api/v1/apps/#{app.id}/usage?months=1") |> json_response(200)
    assert [%{"month" => "2026-10"}] = body["months"]
    assert as(owner) |> get("/api/v1/apps/#{app.id}/usage?months=0") |> json_response(400)
    assert as(owner) |> get("/api/v1/apps/#{app.id}/usage?months=x") |> json_response(400)
  end

  test "the cap API: owner only, JSON only, a whole number or null", %{app: app, owner: owner} do
    path = "/api/v1/apps/#{app.id}/cap"

    assert as(owner) |> put_json(path, %{active_users: 100}) |> json_response(200) ==
             %{"cap" => %{"active_users" => 100}}

    assert %{"cap" => %{"active_users" => 100}} =
             as(owner) |> get("/api/v1/apps/#{app.id}/usage") |> json_response(200)

    assert as(owner) |> put_json(path, %{active_users: nil}) |> json_response(200) ==
             %{"cap" => %{"active_users" => nil}}

    for bad <- [%{active_users: -1}, %{active_users: "10"}, %{active_users: 1.5}, %{}] do
      assert %{"error" => _} = as(owner) |> put_json(path, bad) |> json_response(400)
    end

    # A form can't set it; someone else can't see it; signed out, nothing.
    assert as(owner) |> put(path, %{active_users: 5}) |> json_response(415)
    other = Repo.insert!(%Account{id: "acc_o#{System.unique_integer([:positive])}"})
    assert as(other) |> put_json(path, %{active_users: 5}) |> json_response(404)
    assert as(other) |> get("/api/v1/apps/#{app.id}/usage") |> json_response(404)
    assert build_conn() |> get("/api/v1/apps/#{app.id}/usage") |> json_response(401)
    assert Repo.reload(app).mau_cap == nil
  end
end
