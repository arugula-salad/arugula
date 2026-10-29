defmodule PresenceRelay.Billing.Reporter do
  @moduledoc """
  Tells Stripe's meters what Team apps used (#318's P11), from P9's
  rollups, every `billing_report_ms` (15 minutes) on every node.

  Stripe's Billing Meters sum the meter events a customer sends in a
  billing period. Each of our meters is a cumulative quantity of an app's
  month (`PresenceRelay.Billing.quantities/2`): extra monthly active users
  (past Team's included ones) and call minutes, both of which only go up
  within a month. So each run sends the difference between the quantity
  now and what Stripe has already been told (`billing_reports.reported`),
  and the events' sum is the month's quantity, less where billing began
  (its baseline).

  ## Never twice

  A run has two steps.

  1. **Queue**, for each Team app (one transaction each): take the app's
     transaction-level advisory lock (another node already on it: skip
     it), lock its `billing_reports` rows, read the quantities, and for
     each meter that went up insert a meter event into
     `billing_meter_events` and move `reported` to the new quantity, in
     the same transaction. Either both happen or neither, so a crash or a
     second node can't count a difference twice or lose one. (The
     advisory lock spares the second node the wait; the row locks alone
     would keep it right. Transaction-level locks, as Fly's Postgres
     pools through PgBouncer.)
  2. **Send** every unsent event, each in a transaction holding its row
     (`FOR UPDATE SKIP LOCKED`: two nodes never send the same one at
     once), and mark it sent once Stripe takes it. Each event's
     identifier, `<app>:<yyyy-mm>:<meter>:<quantity>`, is fixed when it's
     queued and is Stripe's identifier too: a node that crashed after
     Stripe took an event but before marking it sends the same event
     again next run, and Stripe drops the duplicate (it keeps
     identifiers at least 24 hours; runs are 15 minutes apart).

  ## Months

  Stripe's periods are calendar months (UTC: Checkout anchors them to
  the first), as P9's are. This month's events are stamped now. For the
  first two days of a month a run also finishes last month, stamping its
  events in last month's final second, for apps already billed then; the
  invoice for a month finalizes about an hour after it ends, so what's
  told after that is accepted but not billed (at most a run's worth).

  An event Stripe refuses is kept with its error and tried again each
  run, until it's older than Stripe takes (35 days).
  """
  use GenServer
  import Ecto.Query
  require Logger

  alias PresenceRelay.{Billing, Repo, Usage}
  alias PresenceRelay.Billing.{AppBilling, MeterEvent, Plans, Report, Stripe}

  # pg_try_advisory_xact_lock(@lock, hashtext(app id)): one node per app.
  @lock 3_290_329
  @final_days 2
  @max_age_days 34
  @batch 500

  def start_link(_), do: GenServer.start_link(__MODULE__, nil, name: __MODULE__)

  @doc """
  One run at `now`: queue what's new, then send everything unsent.
  `%{queued: n, sent: n, failed: n}`. Nothing when billing is off.
  """
  def run(now \\ DateTime.utc_now()) do
    if Billing.enabled?() do
      now = DateTime.truncate(now, :second)
      queued = queue(now)
      {sent, failed} = send_pending(Billing.config(), now)
      %{queued: queued, sent: sent, failed: failed}
    else
      %{queued: 0, sent: 0, failed: 0}
    end
  end

  ## Queue

  @doc "Step 1: meter events for every Team app's new usage. How many were queued."
  def queue(%DateTime{} = now) do
    apps =
      Repo.all(
        from(b in AppBilling,
          where: b.status in ^Billing.reportable() and not is_nil(b.customer_id),
          select: {b.app_id, b.customer_id}
        )
      )

    this = Usage.month(DateTime.to_date(now))
    last = if now.day <= @final_days, do: Date.shift(this, month: -1)

    Enum.reduce(apps, 0, fn {app, customer}, n ->
      case queue_app(app, customer, this, last, now) do
        {:ok, k} ->
          n + k

        {:error, e} ->
          Logger.warning("billing: couldn't queue #{app}'s usage: #{inspect(e)}")
          n
      end
    end)
  end

  defp queue_app(app, customer, this, last, now) do
    Repo.transaction(fn ->
      if locked?(app) do
        # This month, its rows made at the first run (billing began with
        # the month: nothing to leave out); last month only where it was
        # billed.
        start(app, this)
        months = if last && billed?(app, last), do: [last, this], else: [this]
        months |> Enum.map(&queue_month(app, customer, &1, now)) |> Enum.sum()
      else
        0
      end
    end)
  rescue
    e in [DBConnection.ConnectionError, Postgrex.Error] -> {:error, Exception.message(e)}
  end

  defp locked?(app),
    do:
      Repo.query!("SELECT pg_try_advisory_xact_lock($1, hashtext($2))", [@lock, app]).rows ==
        [[true]]

  defp start(app, month) do
    rows = for meter <- Map.keys(Plans.meters()), do: %{app_id: app, month: month, meter: meter}

    Repo.insert_all(Report, rows,
      on_conflict: :nothing,
      conflict_target: [:app_id, :month, :meter]
    )
  end

  defp billed?(app, month),
    do: Repo.exists?(from(r in Report, where: r.app_id == ^app and r.month == ^month))

  defp queue_month(app, customer, month, now) do
    reported =
      Repo.all(
        from(r in Report,
          where: r.app_id == ^app and r.month == ^month,
          lock: "FOR UPDATE",
          select: {r.meter, r.reported}
        )
      )
      |> Map.new()

    # Read after the rows are locked: what another node told is in
    # `reported` already, and these quantities are at least as new.
    q = Billing.quantities(app, month)
    stamp = stamp(month, now)

    for {meter, told} <- reported, q[meter] > told, reduce: 0 do
      n ->
        Repo.insert!(%MeterEvent{
          identifier: "#{app}:#{Calendar.strftime(month, "%Y-%m")}:#{meter}:#{q[meter]}",
          app_id: app,
          customer_id: customer,
          event_name: Plans.event_name(meter),
          value: q[meter] - told,
          timestamp: stamp
        })

        {1, _} =
          Repo.update_all(
            from(r in Report,
              where:
                r.app_id == ^app and r.month == ^month and r.meter == ^meter and
                  r.reported == ^told
            ),
            set: [reported: q[meter]]
          )

        n + 1
    end
  end

  # Now for this month; a month that's over, its final second.
  defp stamp(month, now) do
    if Usage.month(DateTime.to_date(now)) == month,
      do: now,
      else: month |> Date.end_of_month() |> DateTime.new!(~T[23:59:59], "Etc/UTC")
  end

  ## Send

  @doc "Step 2: sends every unsent meter event Stripe would still take. `{sent, failed}`."
  def send_pending(config, %DateTime{} = now) do
    now = DateTime.truncate(now, :second)
    oldest = DateTime.add(now, -@max_age_days * 86_400)

    ids =
      Repo.all(
        from(e in MeterEvent,
          where: is_nil(e.sent_at) and e.timestamp > ^oldest,
          order_by: [e.inserted_at, e.identifier],
          limit: @batch,
          select: e.identifier
        )
      )

    Enum.reduce(ids, {0, 0}, fn id, {sent, failed} ->
      case send_one(config, id, now) do
        :sent -> {sent + 1, failed}
        :skipped -> {sent, failed}
        :failed -> {sent, failed + 1}
      end
    end)
  end

  defp send_one(config, id, now) do
    {:ok, result} =
      Repo.transaction(fn ->
        e =
          Repo.one(
            from(e in MeterEvent,
              where: e.identifier == ^id and is_nil(e.sent_at),
              lock: "FOR UPDATE SKIP LOCKED"
            )
          )

        if e, do: post(config, e, now), else: :skipped
      end)

    result
  rescue
    e in [DBConnection.ConnectionError, Postgrex.Error] ->
      Logger.warning("billing: couldn't send meter event #{id}: #{Exception.message(e)}")
      :failed
  end

  defp post(config, %MeterEvent{} = e, now) do
    params = %{
      event_name: e.event_name,
      identifier: e.identifier,
      timestamp: DateTime.to_unix(e.timestamp),
      payload: %{stripe_customer_id: e.customer_id, value: e.value}
    }

    case Stripe.request(config, :post, "/v1/billing/meter_events", params,
           idempotency_key: e.identifier
         ) do
      {:ok, _} ->
        mark_sent(e, now)

      {:error, {400, body} = err} ->
        # Stripe already has an event with this identifier: ours, sent
        # before a crash.
        if duplicate?(body), do: mark_sent(e, now), else: failed(e, err)

      {:error, err} ->
        failed(e, err)
    end
  end

  defp duplicate?(%{"error" => %{"message" => m}}) when is_binary(m),
    do: m =~ ~r/identifier/i and m =~ ~r/already|duplicate|unique/i

  defp duplicate?(_), do: false

  defp mark_sent(e, now) do
    e |> Ecto.Changeset.change(sent_at: now, last_error: nil) |> Repo.update!()
    :sent
  end

  defp failed(e, err) do
    why = Stripe.describe(err)
    Logger.warning("billing: meter event #{e.identifier} kept for the next run: #{why}")

    e
    |> Ecto.Changeset.change(attempts: e.attempts + 1, last_error: why)
    |> Repo.update!()

    :failed
  end

  ## The timer

  @impl true
  def init(_) do
    schedule()
    {:ok, nil}
  end

  @impl true
  def handle_info(:run, s) do
    try do
      run()
    rescue
      e -> Logger.warning("billing: report run failed: #{Exception.message(e)}")
    end

    schedule()
    {:noreply, s}
  end

  def handle_info(_, s), do: {:noreply, s}

  defp schedule do
    case Application.get_env(:presence_relay, :billing_report_ms, 15 * 60_000) do
      ms when is_integer(ms) -> Process.send_after(self(), :run, ms)
      # `:manual` (tests): only run/1.
      _ -> nil
    end
  end
end
