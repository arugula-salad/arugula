defmodule PresenceRelay.BillingTest do
  # Billing on Stripe (#318's P11) against a fake Stripe on 127.0.0.1:
  # the config the relay refuses, caps by plan, Checkout and the portal,
  # webhooks (signed, recent, once each, the latest state winning), the
  # grace period, the billing card, and usage reaching the meters exactly
  # once. Two nodes reporting at once: billing_cluster_test.exs. Request
  # shapes against stripe-mock: stripe_mock_test.exs.
  use PresenceRelay.RelayCase, async: false
  import Plug.Conn
  import Phoenix.ConnTest
  import Ecto.Query

  alias PresenceRelay.{Apps, Billing, FakeStripe, Repo, Usage}
  alias PresenceRelay.Accounts.Account
  alias PresenceRelay.Billing.{AppBilling, MeterEvent, Reporter, Stripe}
  alias PresenceRelay.Usage.{Day, Member}

  @moduletag :capture_log
  @full "this app is at its monthly limit"
  @secret "whsec_test_fake"

  setup do
    fake = FakeStripe.start()
    Application.put_env(:presence_relay, :stripe, FakeStripe.config(fake))
    Application.put_env(:presence_relay, :starter_mau_cap, 2)
    Usage.reset()

    on_exit(fn ->
      Application.put_env(:presence_relay, :stripe, nil)
      Application.delete_env(:presence_relay, :starter_mau_cap)
      Application.delete_env(:presence_relay, :usage_today)
    end)

    n = System.unique_integer([:positive])
    owner = Repo.insert!(%Account{id: "acc_b#{n}", email: "b#{n}@example.com"})
    {:ok, app, _} = Apps.create(owner, %{"name" => "Acme"})
    %{fake: fake, app: app, owner: owner}
  end

  defp on(day), do: Application.put_env(:presence_relay, :usage_today, day)

  defp member(app, name, room, sub),
    do:
      client_with(node(), name, room, %{
        "token" => token(%{"room" => room, "sub" => sub, "name" => "M"}, app.id)
      })

  defp as(%Account{id: id}), do: build_conn() |> init_test_session(account_id: id)

  defp post_json(conn, path, body \\ %{}),
    do:
      conn
      |> put_req_header("content-type", "application/json")
      |> post(path, Jason.encode!(body))

  # A webhook as Stripe sends it: the raw body, signed at `t`.
  defp webhook(event, opts \\ []) do
    body = Jason.encode!(event)
    t = Keyword.get(opts, :t, System.system_time(:second))
    secret = Keyword.get(opts, :secret, @secret)

    build_conn()
    |> put_req_header("content-type", "application/json")
    |> put_req_header("stripe-signature", Stripe.sign(body, secret, t))
    |> post("/stripe/webhook", body)
  end

  defp event(type, object, id \\ nil),
    do: %{
      "id" => id || "evt_#{System.unique_integer([:positive])}",
      "object" => "event",
      "type" => type,
      "livemode" => false,
      "data" => %{"object" => object}
    }

  defp sub(app, customer, status, id \\ "sub_1"),
    do: %{
      "id" => id,
      "object" => "subscription",
      "customer" => customer,
      "status" => status,
      "cancel_at_period_end" => false,
      "metadata" => %{"app_id" => app.id},
      "items" => %{"data" => [%{"current_period_end" => 1_793_491_200}]}
    }

  # The app subscribed (Checkout, then its webhook), as Stripe would leave it.
  defp subscribe(fake, app, status \\ "active", id \\ "sub_1") do
    {:ok, _} = Billing.checkout(app, "https://arugula.stream")
    cus = Billing.get(app.id).customer_id
    FakeStripe.put_subscription(fake, sub(app, cus, status, id))

    assert webhook(event("checkout.session.completed", %{"subscription" => id}))
           |> json_response(200)

    cus
  end

  # P9's tables, as if people had joined and called (MAU and seconds).
  defp used(app, month, people, day_seconds \\ []) do
    rows =
      for i <- 1..people//1, do: %{app_id: app.id, month: month, member_key: "p#{i}", days: 1}

    for chunk <- Enum.chunk_every(rows, 5_000),
        do: Repo.insert_all(Member, chunk, on_conflict: :nothing)

    for {day, secs} <- day_seconds do
      Repo.insert_all(Day, [%{app_id: app.id, day: day, active_users: 0, call_seconds: secs}],
        on_conflict: [set: [call_seconds: secs]],
        conflict_target: [:app_id, :day]
      )
    end
  end

  test "the relay refuses a live key unless STRIPE_LIVE=1, a key that isn't one, and half a config" do
    ok = %{
      secret_key: "sk_test_abc",
      webhook_secret: "whsec_x",
      prices: %{base: "price_a", mau: "price_b", minutes: "price_c"}
    }

    assert Billing.check_config!(nil) == :ok
    assert Billing.check_config!(ok) == :ok
    assert Billing.check_config!(%{ok | secret_key: "rk_test_abc"}) == :ok

    assert_raise RuntimeError, ~r/live key/, fn ->
      Billing.check_config!(%{ok | secret_key: "sk_live_abc"})
    end

    assert Billing.check_config!(Map.merge(ok, %{secret_key: "sk_live_abc", live: true})) == :ok

    assert_raise RuntimeError, ~r/isn't a Stripe/, fn ->
      Billing.check_config!(%{ok | secret_key: "pk_test_abc"})
    end

    assert_raise RuntimeError, ~r/WEBHOOK/, fn ->
      Billing.check_config!(%{ok | webhook_secret: nil})
    end

    assert_raise RuntimeError, ~r/PRICE/, fn ->
      Billing.check_config!(%{ok | prices: %{base: "x"}})
    end
  end

  test "billing off: caps are the owner's own or none, and nothing to buy", %{
    app: app,
    owner: owner
  } do
    Application.put_env(:presence_relay, :stripe, nil)
    assert Billing.cap(app.id) == nil
    {:ok, app} = Usage.put_cap(app, 7)
    assert Billing.cap(app.id) == 7

    card = as(owner) |> get("/api/v1/apps/#{app.id}/billing") |> json_response(200)
    assert %{"enabled" => false, "plan" => "starter", "can_upgrade" => false} = card
    assert card["cap"] == %{"active_users" => 7}

    assert %{"error" => "billing isn't set up" <> _} =
             as(owner)
             |> post_json("/api/v1/apps/#{app.id}/billing/checkout")
             |> json_response(409)

    body = Jason.encode!(event("customer.subscription.updated", %{"id" => "sub_1"}))

    assert build_conn()
           |> put_req_header("content-type", "application/json")
           |> put_req_header("stripe-signature", Stripe.sign(body, @secret))
           |> post("/stripe/webhook", body)
           |> json_response(404)
  end

  test "Starter's cap at join, the owner's if lower; the system account's apps aren't capped", %{
    app: app
  } do
    r = room()
    on(~D[2026-10-29])
    assert Billing.cap(app.id) == 2
    {:ok, _, _} = member(app, :a, r, "alice")
    {:ok, _, _} = member(app, :b, r, "bob")
    assert {:error, %{reason: @full}} = member(app, :c, r, "carol")

    {:ok, app} = Usage.put_cap(app, 1)
    assert Billing.cap(app.id) == 1
    {:ok, _} = Usage.put_cap(app, 50)
    assert Billing.cap(app.id) == 2

    system =
      Repo.insert!(%Account{id: "acc_sys#{System.unique_integer([:positive])}", kind: "system"})

    {:ok, meet_like, _} = Apps.create(system, %{"name" => "meet"})
    assert Billing.cap(meet_like.id) == nil
  end

  test "Upgrade: the app's customer once, then Checkout with Team's prices from next month", %{
    fake: fake,
    app: app,
    owner: owner
  } do
    path = "/api/v1/apps/#{app.id}/billing/checkout"

    assert %{"url" => "https://checkout.stripe.test/" <> _} =
             as(owner) |> post_json(path) |> json_response(200)

    assert %{"url" => _} = as(owner) |> post_json(path) |> json_response(200)

    assert [c] = FakeStripe.requests(fake, "/v1/customers")
    assert c.params["email"] == owner.email
    assert c.params["metadata"] == %{"app_id" => app.id, "account_id" => owner.id}
    assert c.auth == "Bearer sk_test_fake"

    [s | _] = FakeStripe.requests(fake, "/v1/checkout/sessions")
    p = s.params
    assert p["mode"] == "subscription"
    assert p["customer"] == Billing.get(app.id).customer_id
    assert p["client_reference_id"] == app.id

    assert p["line_items"] == %{
             "0" => %{"price" => "price_base", "quantity" => "1"},
             "1" => %{"price" => "price_mau"},
             "2" => %{"price" => "price_minutes"}
           }

    assert p["subscription_data"]["metadata"] == %{"app_id" => app.id}
    # Managed Payments on, and none of the parameters it refuses.
    assert p["managed_payments"] == %{"enabled" => "true"}

    for refused <-
          ~w(payment_method_types automatic_tax tax_id_collection customer_update invoice_creation adaptive_pricing),
        do: refute(Map.has_key?(p, refused))

    assert s.version == "2026-09-30.endive"
    anchor = String.to_integer(p["subscription_data"]["billing_cycle_anchor"])
    assert anchor == Billing.next_month(DateTime.utc_now()) |> DateTime.to_unix()
    assert {:ok, %DateTime{day: 1, hour: 0, minute: 0}} = DateTime.from_unix(anchor)
    assert p["success_url"] =~ "/dashboard/apps/#{app.id}?billing=done#billing"

    # The portal for its customer; someone else's app, nothing.
    assert %{"url" => "https://billing.stripe.test/p/cus_fake1"} =
             as(owner) |> post_json("/api/v1/apps/#{app.id}/billing/portal") |> json_response(200)

    other = Repo.insert!(%Account{id: "acc_o#{System.unique_integer([:positive])}"})
    assert as(other) |> post_json(path) |> json_response(404)
    {:ok, other_app, _} = Apps.create(other, %{"name" => "Other"})

    assert %{"error" => "this app has nothing to manage" <> _} =
             as(other)
             |> post_json("/api/v1/apps/#{other_app.id}/billing/portal")
             |> json_response(409)

    # A form can't start one.
    assert as(owner) |> post(path, %{}) |> json_response(415)

    # Stripe down: a plain message, no customer half made.
    FakeStripe.fail_next(fake, "/v1/customers", 500)

    assert %{"error" => "Stripe didn't answer" <> _} =
             as(other)
             |> post_json("/api/v1/apps/#{other_app.id}/billing/checkout")
             |> json_response(502)

    refute Billing.get(other_app.id)
  end

  test "webhooks: signed with the secret, recent, once each; Team lifts Starter's cap", %{
    fake: fake,
    app: app,
    owner: owner
  } do
    r = room()
    on(~D[2026-10-29])
    {:ok, _} = Billing.checkout(app, "https://arugula.stream")
    cus = Billing.get(app.id).customer_id
    FakeStripe.put_subscription(fake, sub(app, cus, "active"))
    done = event("checkout.session.completed", %{"subscription" => "sub_1"}, "evt_done")

    assert %{"error" => "bad signature"} =
             webhook(done, secret: "whsec_wrong") |> json_response(400)

    old = System.system_time(:second) - 301
    assert %{"error" => "too old" <> _} = webhook(done, t: old) |> json_response(400)

    assert build_conn()
           |> put_req_header("content-type", "application/json")
           |> post("/stripe/webhook", Jason.encode!(done))
           |> json_response(400)

    assert webhook(%{done | "livemode" => true}) |> json_response(400)
    assert Billing.get(app.id).status == nil

    assert %{"result" => "handled"} = webhook(done) |> json_response(200)

    assert %AppBilling{status: "active", subscription_id: "sub_1", customer_id: ^cus} =
             Billing.get(app.id)

    assert Billing.cap(app.id) == nil

    # Stripe sends it again: nothing more happens (no second fetch).
    fetched = length(FakeStripe.requests(fake, "/v1/subscriptions/sub_1"))
    assert %{"result" => "duplicate"} = webhook(done) |> json_response(200)
    assert length(FakeStripe.requests(fake, "/v1/subscriptions/sub_1")) == fetched

    # Team: past Starter's cap; the owner's own cap still holds.
    for {who, i} <- Enum.with_index(~w(a b c d)), do: {:ok, _, _} = member(app, :"m#{i}", r, who)
    :ok = Usage.flush()
    {:ok, app} = Usage.put_cap(app, 4)
    assert {:error, %{reason: @full}} = member(app, :e, r, "e")

    # Events of no interest are taken and dropped.
    assert %{"result" => "ignored"} =
             webhook(event("customer.created", %{"id" => cus})) |> json_response(200)

    # Stripe can't be reached for the subscription: 500, and Stripe retries.
    FakeStripe.fail_next(fake, "/v1/subscriptions/sub_1", 500)
    retry = event("customer.subscription.updated", %{"id" => "sub_1"}, "evt_retry")
    assert webhook(retry) |> json_response(500)
    assert %{"result" => "handled"} = webhook(retry) |> json_response(200)

    card = as(owner) |> get("/api/v1/apps/#{app.id}/billing") |> json_response(200)
    assert %{"plan" => "team", "can_upgrade" => false, "can_manage" => true} = card
    assert card["subscription"]["status"] == "active"
  end

  test "the latest state wins, whatever order events come in; an old subscription can't undo a new one",
       %{fake: fake, app: app} do
    on(~D[2026-10-29])
    cus = subscribe(fake, app)

    # Canceled at Stripe; a late "created" event still leaves it canceled.
    FakeStripe.put_subscription(fake, sub(app, cus, "canceled"))
    webhook(event("customer.subscription.created", %{"id" => "sub_1"})) |> json_response(200)
    assert Billing.get(app.id).status == "canceled"
    assert Billing.cap(app.id) == 2

    # Subscribed again; then the old one's last word arrives.
    FakeStripe.put_subscription(fake, sub(app, cus, "active", "sub_2"))
    webhook(event("customer.subscription.created", %{"id" => "sub_2"})) |> json_response(200)
    webhook(event("customer.subscription.deleted", %{"id" => "sub_1"})) |> json_response(200)
    assert %AppBilling{subscription_id: "sub_2", status: "active"} = Billing.get(app.id)

    # Someone else's customer can't move this app.
    other =
      Map.put(sub(app, "cus_someone_else", "canceled", "sub_x"), "metadata", %{"app_id" => app.id})

    FakeStripe.put_subscription(fake, other)
    webhook(event("customer.subscription.updated", %{"id" => "sub_x"})) |> json_response(200)
    assert %AppBilling{subscription_id: "sub_2", status: "active"} = Billing.get(app.id)

    # An invoice event (the newer API's shape) finds its subscription too.
    FakeStripe.put_subscription(fake, sub(app, cus, "past_due", "sub_2"))

    inv = %{"id" => "in_1", "parent" => %{"subscription_details" => %{"subscription" => "sub_2"}}}
    webhook(event("invoice.payment_failed", inv)) |> json_response(200)
    assert Billing.get(app.id).status == "past_due"
  end

  test "payments stopped: Team's caps for the grace period, then Starter's, until it's paid", %{
    fake: fake,
    app: app,
    owner: owner
  } do
    on(~D[2026-10-29])
    cus = subscribe(fake, app)
    FakeStripe.put_subscription(fake, sub(app, cus, "past_due"))
    webhook(event("customer.subscription.updated", %{"id" => "sub_1"})) |> json_response(200)

    b = Billing.get(app.id)
    assert b.lapsed_at
    assert Billing.plan(b) == "team"
    assert Billing.cap(app.id) == nil

    card = as(owner) |> get("/api/v1/apps/#{app.id}/billing") |> json_response(200)
    assert {:ok, until, _} = DateTime.from_iso8601(card["subscription"]["grace_until"])
    assert DateTime.diff(until, b.lapsed_at, :day) == 7
    assert card["can_upgrade"] == false

    # Still unpaid (now `unpaid`): the grace runs from when it first stopped.
    FakeStripe.put_subscription(fake, sub(app, cus, "unpaid"))
    webhook(event("customer.subscription.updated", %{"id" => "sub_1"})) |> json_response(200)
    assert Billing.get(app.id).lapsed_at == b.lapsed_at

    # A week and a moment on: Starter's cap.
    eight_days_ago = DateTime.utc_now() |> DateTime.add(-8 * 86_400) |> DateTime.truncate(:second)

    Repo.update_all(from(x in AppBilling, where: x.app_id == ^app.id),
      set: [lapsed_at: eight_days_ago]
    )

    assert Billing.plan(Billing.get(app.id)) == "starter"
    assert Billing.cap(app.id) == 2

    # Paid: Team again, the clock cleared.
    FakeStripe.put_subscription(fake, sub(app, cus, "active"))

    webhook(event("invoice.paid", %{"id" => "in_2", "subscription" => "sub_1"}))
    |> json_response(200)

    assert %AppBilling{status: "active", lapsed_at: nil} = Billing.get(app.id)
    assert Billing.cap(app.id) == nil
  end

  test "the billing card: the plan, the cap, the plans, and this month so far", %{
    fake: fake,
    app: app,
    owner: owner
  } do
    on(~D[2026-10-29])
    oct = ~D[2026-10-01]
    path = "/api/v1/apps/#{app.id}/billing"
    used(app, oct, 2, [{~D[2026-10-02], 125}])

    card = as(owner) |> get(path) |> json_response(200)

    assert %{"enabled" => true, "test_mode" => true, "plan" => "starter", "can_upgrade" => true} =
             card

    assert card["cap"] == %{"active_users" => 2}
    assert card["plans"]["team"]["base"] == "29.00"
    assert card["plans"]["enterprise"]["contact"] == "mailto:hello@arugula.stream"
    assert %{"total" => "0.00", "active_users" => 2, "call_minutes" => 3} = card["estimate"]

    # On Team from now: what came before isn't billed; what comes after is.
    subscribe(fake, app)
    used(app, oct, 520, [{~D[2026-10-02], 125}, {~D[2026-10-29], 60 * 250}])
    card = as(owner) |> get(path) |> json_response(200)

    assert card["estimate"] == %{
             "month" => "2026-10",
             "base" => "29.00",
             "active_users" => 520,
             "included_active_users" => 500,
             "extra_active_users" => 20,
             "extra_active_users_cost" => "1.00",
             "call_minutes" => 250,
             "call_minutes_cost" => "1.00",
             "total" => "31.00"
           }

    assert card["cap"] == %{"active_users" => nil}
    # The usage card shows the cap enforced beside the owner's own.
    assert %{"cap" => %{"active_users" => nil, "limit" => nil}} =
             as(owner) |> get("/api/v1/apps/#{app.id}/usage") |> json_response(200)
  end

  test "usage reaches Stripe's meters once: new usage each run, nothing twice", %{
    fake: fake,
    app: app
  } do
    on(~D[2026-10-10])
    oct = ~D[2026-10-01]
    # Before Team: 3 minutes, not billed.
    used(app, oct, 90, [{~D[2026-10-02], 150}])
    cus = subscribe(fake, app)

    used(app, oct, 503, [{~D[2026-10-02], 150}, {~D[2026-10-10], 600}])
    now = ~U[2026-10-10 12:00:00Z]
    assert %{queued: 2, sent: 2, failed: 0} = Reporter.run(now)

    assert [
             %{event_name: "presence_extra_active_users", value: 3, customer: ^cus} = mau,
             %{event_name: "presence_call_minutes", value: 10} = mins
           ] = FakeStripe.meter_events(fake) |> Enum.sort_by(& &1.event_name, :desc)

    assert mau.identifier == "#{app.id}:2026-10:mau:3"
    assert mins.identifier == "#{app.id}:2026-10:minutes:13"
    assert mau.timestamp == DateTime.to_unix(now)

    # Nothing new: nothing sent. A restart (a new node) changes nothing.
    assert %{queued: 0, sent: 0} = Reporter.run(now)
    assert %{queued: 0, sent: 0} = Reporter.run(DateTime.add(now, 900))

    # More: only the difference.
    used(app, oct, 510, [{~D[2026-10-10], 601}])
    assert %{queued: 2, sent: 2} = Reporter.run(DateTime.add(now, 1800))
    assert FakeStripe.total(fake, "presence_extra_active_users", cus) == 10
    assert FakeStripe.total(fake, "presence_call_minutes", cus) == 11
    assert Repo.aggregate(from(e in MeterEvent, where: is_nil(e.sent_at)), :count) == 0

    # A Starter app sends nothing.
    {:ok, starter, _} = Apps.create(Repo.get!(Account, app.owner_id), %{"name" => "Free"})
    used(starter, oct, 700)
    assert %{queued: 0} = Reporter.run(DateTime.add(now, 2700))
  end

  test "a crash after Stripe took an event: the resend is the same event, Stripe keeps one", %{
    fake: fake,
    app: app
  } do
    on(~D[2026-10-10])
    oct = ~D[2026-10-01]
    cus = subscribe(fake, app)
    used(app, oct, 0, [{~D[2026-10-10], 300}])
    now = ~U[2026-10-10 12:00:00Z]

    # Stripe takes it, but we crash before marking it sent.
    assert %{sent: 1} = Reporter.run(now)
    Repo.update_all(MeterEvent, set: [sent_at: nil])
    assert %{queued: 0, sent: 1} = Reporter.run(now)
    assert FakeStripe.total(fake, "presence_call_minutes", cus) == 5
    assert length(FakeStripe.meter_events(fake)) == 1

    # Each send carries the identifier as its idempotency key too.
    assert Enum.all?(
             FakeStripe.requests(fake, "/v1/billing/meter_events"),
             &(&1.idempotency_key == &1.params["identifier"])
           )

    # Stripe down: kept, with why, and sent next run.
    used(app, oct, 0, [{~D[2026-10-10], 420}])
    FakeStripe.fail_next(fake, "/v1/billing/meter_events", 503)
    assert %{queued: 1, sent: 0, failed: 1} = Reporter.run(DateTime.add(now, 900))

    assert [%MeterEvent{attempts: 1, last_error: "Stripe said 503" <> _}] =
             Repo.all(from(e in MeterEvent, where: is_nil(e.sent_at)))

    assert %{queued: 0, sent: 1} = Reporter.run(DateTime.add(now, 1800))
    assert FakeStripe.total(fake, "presence_call_minutes", cus) == 7

    # Two runs racing on one node's view (a stale read): the row locks and
    # the identifier keep it to one.
    used(app, oct, 0, [{~D[2026-10-10], 600}])

    results =
      for _ <- 1..4 do
        Task.async(fn -> Reporter.queue(DateTime.add(now, 2700)) end)
      end
      |> Task.await_many()

    assert Enum.sum(results) == 1
    Reporter.run(DateTime.add(now, 2700))
    assert FakeStripe.total(fake, "presence_call_minutes", cus) == 10
  end

  test "a month's end: the next month's first runs finish it, stamped in its last second", %{
    fake: fake,
    app: app
  } do
    on(~D[2026-10-30])
    cus = subscribe(fake, app)
    used(app, ~D[2026-10-01], 0, [{~D[2026-10-31], 120}])
    assert %{sent: 1} = Reporter.run(~U[2026-10-31 23:50:00Z])

    # The last minutes of October land after its last run.
    used(app, ~D[2026-10-01], 0, [{~D[2026-10-31], 300}])
    used(app, ~D[2026-11-01], 0, [{~D[2026-11-01], 60}])
    assert %{queued: 2, sent: 2} = Reporter.run(~U[2026-11-01 00:10:00Z])

    events = FakeStripe.meter_events(fake)
    oct_last = Enum.find(events, &(&1.identifier == "#{app.id}:2026-10:minutes:5"))
    assert oct_last.value == 3
    assert oct_last.timestamp == DateTime.to_unix(~U[2026-10-31 23:59:59Z])
    nov = Enum.find(events, &(&1.identifier == "#{app.id}:2026-11:minutes:1"))
    assert nov.timestamp == DateTime.to_unix(~U[2026-11-01 00:10:00Z])
    assert FakeStripe.total(fake, "presence_call_minutes", cus) == 6

    # From the third, October is left alone.
    used(app, ~D[2026-10-01], 0, [{~D[2026-10-31], 900}])
    assert %{queued: 0} = Reporter.run(~U[2026-11-03 00:10:00Z])
  end

  test "Stripe-Signature: any v1 that matches, within five minutes either way" do
    body = ~s({"id":"evt_1"})
    t = 1_800_000_000
    good = Stripe.sign(body, "whsec_a", t)
    "t=" <> _ = good
    [_, v1] = String.split(good, ",")

    assert Stripe.verify(body, good, "whsec_a", t) == :ok
    assert Stripe.verify(body, "t=#{t},v1=deadbeef,#{v1}", "whsec_a", t + 300) == :ok
    assert {:error, "too old" <> _} = Stripe.verify(body, good, "whsec_a", t + 301)
    assert {:error, "too old" <> _} = Stripe.verify(body, good, "whsec_a", t - 301)
    assert {:error, "bad signature"} = Stripe.verify(body <> " ", good, "whsec_a", t)
    assert {:error, "bad signature"} = Stripe.verify(body, good, "whsec_b", t)
    assert {:error, _} = Stripe.verify(body, "v1=#{v1}", "whsec_a", t)
    assert {:error, _} = Stripe.verify(body, "t=#{t}", "whsec_a", t)
    assert {:error, _} = Stripe.verify(body, nil, "whsec_a", t)
  end

  test "Stripe's form encoding" do
    assert Stripe.encode(%{
             b: [%{price: "p1", quantity: 1}, %{price: "p2"}],
             a: %{x: "1 2", y: nil},
             c: true
           }) ==
             "a%5Bx%5D=1+2&b%5B0%5D%5Bprice%5D=p1&b%5B0%5D%5Bquantity%5D=1&b%5B1%5D%5Bprice%5D=p2&c=true"
  end
end
