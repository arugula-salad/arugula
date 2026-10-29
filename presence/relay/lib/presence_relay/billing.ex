defmodule PresenceRelay.Billing do
  @moduledoc """
  Billing on Stripe (#318's P11): each app on a plan, Team's usage billed
  by Stripe's meters, and caps by plan. The numbers are in
  `PresenceRelay.Billing.Plans`.

  ## Per app

  Each app is on a plan, as P9 meters each app: Starter (free, capped),
  Team (a subscription), or Enterprise (by talking to us, no self-serve).
  An app on Team has a Stripe customer of its own, made at its first
  Upgrade with the owner's address: Stripe's meters count per customer,
  so a customer per app keeps one app's usage off another's
  subscription. (An owner with two Team apps enters a card twice, and
  gets two invoices, one an app.)

  ## Test mode only

  `STRIPE_SECRET_KEY` must be a test key (`sk_test_`, or a restricted
  `rk_test_`). A live key refuses to boot unless `STRIPE_LIVE=1`, which
  Jake sets when he goes live (`check_config!/1`). With no Stripe config
  billing is off: no plans, no Upgrade, and caps are what they were
  before billing (the owner's own, else none).

  ## Subscribing

  Upgrade is Stripe Checkout (`checkout/3`): the base price and the two
  metered prices, its billing anchored to the first of next month (UTC),
  so Stripe's periods are P9's calendar months; the first, part month's
  base is prorated. Manage is Stripe's Customer Portal (`portal/2`):
  card, invoices, cancel.

  ## Managed Payments

  Built for Stripe as merchant of record (Managed Payments: it handles
  tax, receipts and payment support, from Link). So subscriptions are
  made only by Checkout Sessions with `managed_payments[enabled]`
  (config `managed_payments`, `STRIPE_MANAGED_PAYMENTS=0` turns it off),
  never the Subscriptions API; usage is billed only by metered prices on
  Billing Meters, never invoice items; no invoice is previewed or made
  here (the dashboard's estimate is our own arithmetic on P9's numbers);
  the card is changed in the Customer Portal; and the products carry tax
  code `txcd_10103001` (SaaS, business use).

  Stripe's webhooks (`handle_event/1`, at `POST /stripe/webhook`) keep
  each app's subscription in `app_billing`, found by its customer (else
  the app id Checkout put in its metadata). Each event is handled once,
  by id (`stripe_events`); whatever the event, the relay fetches the
  subscription it's about and keeps Stripe's current state, so events
  arriving out of order or twice can't leave an old state behind.

  ## Caps by plan

  - **Starter:** `Plans.starter_mau_cap/0`, or the owner's cap if lower.
  - **Team:** none of ours; the owner's own (P9) if they set one.
  - **Payments stopped** (`past_due`, `unpaid`, `paused`): Team's caps
    for `Plans.grace_days/0` (7) from when they stopped, then
    Starter's, until it's paid. Canceled: Starter's at once (a cancel
    takes effect at the end of the paid month).
  - **The system account's apps** (`meet`) are never capped by plan.
  - **Billing off:** the owner's cap, else none (as before P11).

  Each node's `PresenceRelay.Usage` caches caps for 30 s; a webhook that
  changes an app's subscription tells every node to forget it.
  """

  import Ecto.Query
  require Logger

  alias PresenceRelay.{Accounts, Apps, Repo, Usage}
  alias PresenceRelay.Accounts.Account
  alias PresenceRelay.Apps.App
  alias PresenceRelay.Billing.{AppBilling, Plans, Report, Stripe}
  alias PresenceRelay.Usage.{Day, Member}

  # Paid up: Team.
  @good ~w(active trialing)
  # Payments stopped: Team for the grace period, then Starter.
  @lapsed ~w(past_due unpaid paused)
  # Billed for usage (Stripe still invoices these).
  @reportable ~w(active trialing past_due unpaid)

  ## Config

  # The API version every request asks for (Managed Payments needs
  # 2025-03-31.basil or later); STRIPE_API_VERSION overrides.
  @api_version "2026-09-30.endive"

  @doc """
  The Stripe config, or nil (billing off): `%{secret_key, webhook_secret,
  prices: %{base, mau, minutes}, api_base, api_version, managed_payments,
  live}`.
  """
  def config do
    case Application.get_env(:presence_relay, :stripe) do
      %{secret_key: k} = c when is_binary(k) ->
        c
        |> Map.put_new(:api_base, "https://api.stripe.com")
        |> Map.put_new(:api_version, @api_version)
        |> Map.put_new(:managed_payments, true)

      _ ->
        nil
    end
  end

  @doc "Whether billing is on: Stripe config and a database."
  def enabled?, do: config() != nil and Apps.database?()

  @doc "Whether the key is a test one (always, unless Jake set `STRIPE_LIVE=1`)."
  def test_mode? do
    case config() do
      %{secret_key: k} -> test_key?(k)
      nil -> true
    end
  end

  defp test_key?(k), do: String.starts_with?(k, ["sk_test_", "rk_test_"])
  defp live_key?(k), do: String.starts_with?(k, ["sk_live_", "rk_live_"])

  @doc """
  Refuses (raises) a config the relay mustn't boot with: a live key
  without `live: true` (`STRIPE_LIVE=1`), a key that's neither test nor
  live, or a missing webhook secret or price. nil (billing off) is fine.
  """
  def check_config!(nil), do: :ok

  def check_config!(%{secret_key: key} = c) do
    cond do
      live_key?(key) and c[:live] != true ->
        raise "STRIPE_SECRET_KEY is a live key: billing is test mode only until STRIPE_LIVE=1"

      not test_key?(key) and not live_key?(key) ->
        raise "STRIPE_SECRET_KEY isn't a Stripe secret key (sk_test_…)"

      not is_binary(c[:webhook_secret]) or c[:webhook_secret] == "" ->
        raise "STRIPE_WEBHOOK_SECRET is missing"

      Enum.any?([:base, :mau, :minutes], &(not is_binary(get_in(c, [:prices, &1])))) ->
        raise "STRIPE_PRICE_TEAM_BASE, STRIPE_PRICE_TEAM_MAU and STRIPE_PRICE_TEAM_MINUTES are needed (scripts/stripe_setup.exs prints them)"

      true ->
        :ok
    end
  end

  ## Plans and caps

  @doc "The app's billing row, or nil (never in Stripe)."
  def get(app_id), do: Repo.get(AppBilling, app_id)

  @doc """
  The plan an app's billing row puts it on at `now`: `"team"` (paid up,
  or lapsed within the grace period) or `"starter"`.
  """
  def plan(b, now \\ DateTime.utc_now())

  def plan(%AppBilling{status: s}, _) when s in @good, do: "team"

  def plan(%AppBilling{status: s, lapsed_at: %DateTime{} = at}, now) when s in @lapsed,
    do: if(DateTime.compare(now, grace_until(at)) == :lt, do: "team", else: "starter")

  def plan(_, _), do: "starter"

  defp grace_until(%DateTime{} = at), do: DateTime.add(at, Plans.grace_days() * 86_400)

  @doc """
  The cap on monthly active users the relay enforces for `app` (an id):
  by plan when billing is on, and never above the owner's own. nil for
  none. What `PresenceRelay.Usage` checks at join.
  """
  def cap(app_id) do
    from(a in App,
      join: o in Account,
      on: o.id == a.owner_id,
      left_join: b in AppBilling,
      on: b.app_id == a.id,
      where: a.id == ^app_id,
      select: {a.mau_cap, o.kind, b}
    )
    |> Repo.one()
    |> case do
      nil -> nil
      {own, kind, b} -> limit(own, kind, b, DateTime.utc_now())
    end
  end

  defp limit(own, kind, b, now) do
    if enabled?() and kind != "system" and plan(b, now) == "starter",
      do: min_cap(own, Plans.starter_mau_cap()),
      else: own
  end

  defp min_cap(nil, plan), do: plan
  defp min_cap(own, plan), do: min(own, plan)

  ## What the dashboard shows

  @doc """
  The app's billing card (`GET /api/v1/apps/:id/billing`): whether
  billing is on and in test mode, its plan and subscription, its cap,
  the plans, and what this month costs so far.
  """
  def view(%App{} = app, now \\ DateTime.utc_now()) do
    b = get(app.id)
    owner = Accounts.get(app.owner_id)
    kind = if owner, do: owner.kind, else: "user"
    on? = enabled?()
    plan = plan(b, now)
    month = Usage.month(Usage.today())

    %{
      enabled: on?,
      test_mode: test_mode?(),
      managed_payments: on? and config().managed_payments == true,
      plan: plan,
      subscription: b && sub_view(b),
      cap: %{active_users: limit(app.mau_cap, kind, b, now)},
      plans: Plans.view(),
      estimate: estimate(app.id, plan, b, month),
      can_upgrade: on? and plan == "starter" and not lapsed?(b),
      can_manage: on? and not is_nil(b && b.customer_id)
    }
  end

  defp sub_view(%AppBilling{} = b) do
    %{
      status: b.status,
      cancel_at_period_end: b.cancel_at_period_end,
      current_period_end: b.current_period_end,
      grace_until: if(lapsed?(b) and b.lapsed_at != nil, do: grace_until(b.lapsed_at))
    }
  end

  defp lapsed?(%AppBilling{status: s}), do: s in @lapsed
  defp lapsed?(_), do: false

  @doc """
  This month so far, priced: Team's base, extra active users past the
  included ones, and call minutes, each since billing began this month
  (before Stripe's proration of a first, part month, and before tax).
  Starter's is nothing.
  """
  def estimate(app_id, plan, b, month) do
    q = quantities(app_id, month)
    zero = Decimal.new(0)

    if plan == "team" or (b != nil and b.status in @reportable) do
      base = from_baseline(app_id, month)
      extra = max(q["mau"] - base["mau"], 0)
      minutes = max(q["minutes"] - base["minutes"], 0)
      extra_cost = Decimal.mult(extra, Plans.team_per_extra_mau())
      minutes_cost = Decimal.mult(minutes, Plans.team_per_call_minute())
      total = Plans.team_base() |> Decimal.add(extra_cost) |> Decimal.add(minutes_cost)

      %{
        month: Calendar.strftime(month, "%Y-%m"),
        base: money(Plans.team_base()),
        active_users: q[:active_users],
        included_active_users: Plans.team_included_mau(),
        extra_active_users: extra,
        extra_active_users_cost: money(extra_cost),
        call_minutes: minutes,
        call_minutes_cost: money(minutes_cost),
        total: money(total)
      }
    else
      %{
        month: Calendar.strftime(month, "%Y-%m"),
        base: money(zero),
        active_users: q[:active_users],
        included_active_users: nil,
        extra_active_users: 0,
        extra_active_users_cost: money(zero),
        call_minutes: q["minutes"],
        call_minutes_cost: money(zero),
        total: money(zero)
      }
    end
  end

  defp from_baseline(app_id, month) do
    rows =
      Repo.all(
        from(r in Report,
          where: r.app_id == ^app_id and r.month == ^month,
          select: {r.meter, r.baseline}
        )
      )
      |> Map.new()

    %{"mau" => Map.get(rows, "mau", 0), "minutes" => Map.get(rows, "minutes", 0)}
  end

  @doc "Dollars as a string with cents: `\"29.00\"`."
  def money(d), do: d |> Decimal.round(2) |> Decimal.to_string(:normal)

  @doc """
  An app's month as Stripe's meters count it, from P9's rollups: `"mau"`,
  its monthly active users past Team's included ones, and `"minutes"`,
  its call minutes (each day's seconds rounded up, as P9 reports them);
  and `:active_users`, all of them. Each only goes up within a month.
  """
  def quantities(app_id, %Date{} = month) do
    mau =
      Repo.aggregate(from(u in Member, where: u.app_id == ^app_id and u.month == ^month), :count)

    last = Date.end_of_month(month)

    minutes =
      Repo.one(
        from(d in Day,
          where: d.app_id == ^app_id and d.day >= ^month and d.day <= ^last,
          select: coalesce(sum(fragment("(? + 59) / 60", d.call_seconds)), 0)
        )
      )

    %{
      :active_users => mau,
      "mau" => max(mau - Plans.team_included_mau(), 0),
      "minutes" => to_int(minutes)
    }
  end

  defp to_int(%Decimal{} = d), do: Decimal.to_integer(d)
  defp to_int(n) when is_integer(n), do: n

  ## Checkout and the portal

  @doc """
  A Stripe Checkout page subscribing `app` to Team: `{:ok, url}` or
  `{:error, status, message}`. Makes the app's customer first, once.
  `base_url` is the dashboard's own origin, for coming back.
  """
  def checkout(%App{} = app, base_url, now \\ DateTime.utc_now()) do
    with {:ok, c} <- ready(),
         :ok <- can_upgrade(app),
         {:ok, customer} <- customer(c, app) do
      back = "#{base_url}/dashboard/apps/#{app.id}"

      params = %{
        mode: "subscription",
        customer: customer,
        client_reference_id: app.id,
        line_items: [
          %{price: c.prices.base, quantity: 1},
          %{price: c.prices.mau},
          %{price: c.prices.minutes}
        ],
        success_url: back <> "?billing=done#billing",
        cancel_url: back <> "?billing=cancelled#billing",
        metadata: %{app_id: app.id},
        subscription_data: %{
          metadata: %{app_id: app.id},
          # Periods are calendar months (UTC), as P9 counts them; the part
          # month until then is prorated.
          billing_cycle_anchor: next_month(now) |> DateTime.to_unix(),
          proration_behavior: "create_prorations"
        }
      }

      # Stripe as merchant of record (tax, receipts from Link): on unless
      # STRIPE_MANAGED_PAYMENTS=0. Nothing above is a parameter it refuses
      # (payment_method_types, automatic_tax, tax_id_collection, ...).
      params =
        if c.managed_payments,
          do: Map.put(params, :managed_payments, %{enabled: true}),
          else: params

      case Stripe.request(c, :post, "/v1/checkout/sessions", params) do
        {:ok, %{"url" => url}} when is_binary(url) -> {:ok, url}
        {:ok, _} -> {:error, 502, "Stripe didn't give a checkout page"}
        {:error, e} -> stripe_error(e)
      end
    end
  end

  @doc "A Stripe Customer Portal page for `app`'s customer: `{:ok, url}` or `{:error, status, message}`."
  def portal(%App{} = app, base_url) do
    with {:ok, c} <- ready() do
      case get(app.id) do
        %AppBilling{customer_id: cus} when is_binary(cus) ->
          params = %{customer: cus, return_url: "#{base_url}/dashboard/apps/#{app.id}#billing"}

          case Stripe.request(c, :post, "/v1/billing_portal/sessions", params) do
            {:ok, %{"url" => url}} when is_binary(url) -> {:ok, url}
            {:ok, _} -> {:error, 502, "Stripe didn't give a portal page"}
            {:error, e} -> stripe_error(e)
          end

        _ ->
          {:error, 409, "this app has nothing to manage yet: upgrade it first"}
      end
    end
  end

  defp ready do
    if enabled?(), do: {:ok, config()}, else: {:error, 409, "billing isn't set up on this relay"}
  end

  defp can_upgrade(app) do
    case get(app.id) do
      %AppBilling{status: s} when s in @reportable or s == "paused" ->
        {:error, 409, "this app already has a subscription: manage it instead"}

      _ ->
        :ok
    end
  end

  defp stripe_error(e) do
    Logger.warning("billing: #{Stripe.describe(e)}")
    {:error, 502, "Stripe didn't answer as expected; try again in a minute"}
  end

  # The app's Stripe customer, made once (the row locked while we ask, so
  # two clicks make one).
  defp customer(c, app) do
    Repo.transaction(fn ->
      now = DateTime.utc_now() |> DateTime.truncate(:second)

      Repo.insert_all(AppBilling, [%{app_id: app.id, inserted_at: now, updated_at: now}],
        on_conflict: :nothing,
        conflict_target: :app_id
      )

      b = Repo.one!(from(b in AppBilling, where: b.app_id == ^app.id, lock: "FOR UPDATE"))

      case b.customer_id do
        nil ->
          owner = Accounts.get(app.owner_id)

          params = %{
            name: "#{app.name} (#{app.id})",
            email: owner && owner.email,
            metadata: %{app_id: app.id, account_id: app.owner_id}
          }

          case Stripe.request(c, :post, "/v1/customers", params,
                 idempotency_key: "customer-#{app.id}"
               ) do
            {:ok, %{"id" => cus}} ->
              b |> Ecto.Changeset.change(customer_id: cus) |> Repo.update!()
              cus

            {:error, e} ->
              Repo.rollback(e)
          end

        cus ->
          cus
      end
    end)
    |> case do
      {:ok, cus} -> {:ok, cus}
      {:error, e} -> stripe_error(e)
    end
  end

  @doc "The first instant of the month after `now`'s (UTC)."
  def next_month(%DateTime{} = now) do
    now
    |> DateTime.to_date()
    |> Date.end_of_month()
    |> Date.add(1)
    |> DateTime.new!(~T[00:00:00], "Etc/UTC")
  end

  ## Webhooks

  @doc """
  One verified webhook event: `{:ok, :handled | :duplicate | :ignored}`, or
  `{:error, why}` (Stripe retries it). Handled once by id: the event's id
  goes in with whatever it changed, in one transaction.
  """
  def handle_event(%{"id" => id, "type" => type} = event)
      when is_binary(id) and is_binary(type) do
    c = config()
    now = DateTime.utc_now() |> DateTime.truncate(:second)

    Repo.transaction(fn ->
      case Repo.insert_all("stripe_events", [%{id: id, type: type, inserted_at: now}],
             on_conflict: :nothing,
             conflict_target: :id
           ) do
        {0, _} ->
          {:duplicate, nil}

        {1, _} ->
          case dispatch(c, type, get_in(event, ["data", "object"]) || %{}) do
            {:ok, nil} -> {:ignored, nil}
            {:ok, app_id} -> {:handled, app_id}
            {:error, why} -> Repo.rollback(why)
          end
      end
    end)
    |> case do
      {:ok, {result, app_id}} ->
        if app_id, do: Usage.forget_everywhere(app_id)
        {:ok, result}

      {:error, why} ->
        {:error, why}
    end
  end

  def handle_event(_), do: {:error, "not an event"}

  defp dispatch(c, "checkout.session." <> what, obj)
       when what in ~w(completed async_payment_succeeded),
       do: sync(c, id_of(obj["subscription"]))

  defp dispatch(c, "customer.subscription." <> _, obj), do: sync(c, obj["id"])

  defp dispatch(c, "invoice." <> what, obj)
       when what in ~w(paid payment_failed payment_succeeded),
       do:
         sync(
           c,
           id_of(obj["subscription"]) ||
             id_of(get_in(obj, ["parent", "subscription_details", "subscription"]))
         )

  defp dispatch(_, _, _), do: {:ok, nil}

  defp id_of(%{"id" => id}), do: id
  defp id_of(id) when is_binary(id), do: id
  defp id_of(_), do: nil

  # The subscription as Stripe has it now, kept.
  defp sync(_, nil), do: {:ok, nil}

  defp sync(c, sub_id) when is_binary(sub_id) do
    case Stripe.request(c, :get, "/v1/subscriptions/#{URI.encode_www_form(sub_id)}") do
      {:ok, sub} -> apply_subscription(sub)
      {:error, e} -> {:error, Stripe.describe(e)}
    end
  end

  @doc """
  Keeps a Stripe subscription (as fetched) on its app (`metadata.app_id`):
  `{:ok, app_id}`, or `{:ok, nil}` when it isn't one of ours to keep.
  When it starts being billed, billing begins from this month's usage
  so far (`begin/2`). Called inside a transaction.
  """
  def apply_subscription(%{"id" => sub_id} = sub) do
    customer = id_of(sub["customer"])
    app_id = app_for(customer, get_in(sub, ["metadata", "app_id"]))
    status = sub["status"]

    with true <- is_binary(app_id),
         old = Repo.one(from(b in AppBilling, where: b.app_id == ^app_id, lock: "FOR UPDATE")),
         :ok <- ours(old, sub_id, customer, status) do
      now = DateTime.utc_now() |> DateTime.truncate(:second)

      lapsed_at =
        cond do
          status in @good -> nil
          old && old.lapsed_at && old.status in @lapsed -> old.lapsed_at
          status in @lapsed -> now
          true -> nil
        end

      row = %{
        app_id: app_id,
        customer_id: customer,
        subscription_id: sub_id,
        status: status,
        cancel_at_period_end: sub["cancel_at_period_end"] == true,
        current_period_end: period_end(sub),
        lapsed_at: lapsed_at,
        inserted_at: now,
        updated_at: now
      }

      Repo.insert_all(AppBilling, [row],
        on_conflict: {:replace_all_except, [:app_id, :inserted_at]},
        conflict_target: :app_id
      )

      was_billed = old != nil and old.subscription_id == sub_id and old.status in @reportable
      if status in @reportable and not was_billed, do: begin(app_id, Usage.month(Usage.today()))
      {:ok, app_id}
    else
      _ ->
        Logger.info("billing: subscription #{sub_id} isn't one of ours to keep; ignored")
        {:ok, nil}
    end
  end

  # The app a subscription is for: by its customer, each app's own (the
  # Stripe object that is the app, as Stripe advises), else by the app id
  # Checkout put in its metadata, for an app that exists.
  defp app_for(customer, meta_app) do
    by_customer =
      is_binary(customer) &&
        Repo.one(from(b in AppBilling, where: b.customer_id == ^customer, select: b.app_id))

    cond do
      is_binary(by_customer) -> by_customer
      is_binary(meta_app) and Repo.exists?(from(a in App, where: a.id == ^meta_app)) -> meta_app
      true -> nil
    end
  end

  # An app's own customer only; and an old subscription's last word
  # (canceled, say) doesn't replace the one being billed now.
  defp ours(nil, _, _, _), do: :ok

  defp ours(%AppBilling{} = b, sub_id, customer, status) do
    cond do
      b.customer_id != nil and b.customer_id != customer ->
        :not_ours

      b.subscription_id != nil and b.subscription_id != sub_id and b.status in @reportable and
          status not in @reportable ->
        :stale

      true ->
        :ok
    end
  end

  # Stripe moved the period onto the items (API 2025-03-31); either.
  defp period_end(sub) do
    t =
      sub["current_period_end"] ||
        get_in(sub, ["items", "data", Access.at(0), "current_period_end"])

    if is_integer(t), do: DateTime.from_unix!(t)
  end

  @doc """
  Billing for `app` begins now, in `month`: each meter's baseline (and
  what's been told, if less) is the month's usage so far, so what came
  before the subscription isn't billed.
  """
  def begin(app_id, %Date{} = month) do
    q = quantities(app_id, month)

    for meter <- Map.keys(Plans.meters()) do
      Repo.insert_all(
        Report,
        [%{app_id: app_id, month: month, meter: meter, baseline: q[meter], reported: q[meter]}],
        on_conflict:
          from(r in Report,
            update: [
              set: [
                baseline: fragment("GREATEST(?, EXCLUDED.baseline)", r.reported),
                reported: fragment("GREATEST(?, EXCLUDED.reported)", r.reported)
              ]
            ]
          ),
        conflict_target: [:app_id, :month, :meter]
      )
    end

    :ok
  end

  @doc false
  def reportable, do: @reportable
end
