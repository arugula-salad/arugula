defmodule PresenceRelay.BillingClusterTest do
  # Two nodes telling Stripe's meters at once (#318's P11): each round
  # adds usage, then both nodes run the reporter together, several times
  # over. Each has its own Postgres connections (the peer has no sandbox),
  # so the advisory and row locks are real. Stripe (the fake) must end up
  # with exactly the month's quantities, each event once.
  #
  # Outside the sandbox: it deletes what it made.
  use ExUnit.Case, async: false

  alias PresenceRelay.{Apps, Billing, Cluster, FakeStripe, Repo}
  alias PresenceRelay.Accounts.Account
  alias PresenceRelay.Billing.{AppBilling, Reporter}
  alias PresenceRelay.Usage.{Day, Member}

  @moduletag :cluster
  @moduletag :capture_log
  @oct ~D[2026-10-01]

  setup_all do
    Ecto.Adapters.SQL.Sandbox.mode(Repo, :auto)
    fake = FakeStripe.start()
    Application.put_env(:presence_relay, :stripe, FakeStripe.config(fake))
    # The peer starts with this node's config: billing on, the same fake.
    {peer, node} = Cluster.start_peer!(database: true)

    on_exit(fn ->
      # Gone already if this module's supervisor (the fake's) took it down.
      if Process.alive?(peer), do: :peer.stop(peer)
      Application.put_env(:presence_relay, :stripe, nil)
      Ecto.Adapters.SQL.Sandbox.mode(Repo, :manual)
    end)

    %{peer: node, fake: fake}
  end

  setup do
    n = System.unique_integer([:positive])
    owner = Repo.insert!(%Account{id: "acc_bc#{n}", email: "bc#{n}@example.com"})
    {:ok, app, _} = Apps.create(owner, %{"name" => "Busy"})
    cus = "cus_cluster#{n}"

    Repo.insert!(%AppBilling{
      app_id: app.id,
      customer_id: cus,
      subscription_id: "sub_cluster#{n}",
      status: "active"
    })

    :ok = Billing.begin(app.id, @oct)

    on_exit(fn ->
      # Its usage, reports and meter events go with it (on delete cascade).
      Repo.delete!(app)
      Repo.delete!(owner)
    end)

    %{app: app, cus: cus}
  end

  test "both nodes reporting at once: every event once, the sums exact", %{
    peer: peer,
    fake: fake,
    app: app,
    cus: cus
  } do
    rounds = 12

    for i <- 1..rounds do
      # 480 people, then 9 more a round (past Team's 500 from the third);
      # a day's call seconds growing by 97 a round.
      people =
        for k <- 1..(480 + 9 * i),
            do: %{app_id: app.id, month: @oct, member_key: "p#{k}", days: 1}

      Repo.insert_all(Member, people, on_conflict: :nothing)

      Repo.insert_all(
        Day,
        [%{app_id: app.id, day: ~D[2026-10-15], active_users: 0, call_seconds: 97 * i}],
        on_conflict: [set: [call_seconds: 97 * i]],
        conflict_target: [:app_id, :day]
      )

      now = DateTime.add(~U[2026-10-15 12:00:00Z], i * 60)

      # Three runs on each node, all at once.
      runs =
        for _ <- 1..3, n <- [node(), peer] do
          Task.async(fn -> :erpc.call(n, Reporter, :run, [now], 30_000) end)
        end

      Task.await_many(runs, 60_000)
    end

    q = Billing.quantities(app.id, @oct)
    assert q["mau"] == 480 + 9 * rounds - 500
    assert q["minutes"] == div(97 * rounds + 59, 60)

    assert FakeStripe.total(fake, "presence_extra_active_users", cus) == q["mau"]
    assert FakeStripe.total(fake, "presence_call_minutes", cus) == q["minutes"]

    # Every event Stripe took was sent once: no identifier came twice.
    mine =
      Enum.filter(
        FakeStripe.requests(fake, "/v1/billing/meter_events"),
        &(&1.params["payload"]["stripe_customer_id"] == cus)
      )

    ids = Enum.map(mine, & &1.params["identifier"])
    assert ids == Enum.uniq(ids)
  end
end
