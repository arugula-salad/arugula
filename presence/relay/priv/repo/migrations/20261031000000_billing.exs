defmodule PresenceRelay.Repo.Migrations.Billing do
  use Ecto.Migration

  def change do
    # Each app's place in Stripe (#318 P11): a customer of its own (made at
    # its first Upgrade), and its subscription as the webhooks last told
    # us. No row: Starter, never in Stripe.
    create table(:app_billing, primary_key: false) do
      add :app_id, references(:apps, type: :string, on_delete: :delete_all), primary_key: true
      add :customer_id, :string
      add :subscription_id, :string
      # Stripe's status: active, trialing, past_due, unpaid, canceled, ...
      add :status, :string
      add :cancel_at_period_end, :boolean, null: false, default: false
      add :current_period_end, :utc_datetime
      # When it stopped being paid up (past_due or unpaid): the grace
      # period runs from here. Null while it's in good standing.
      add :lapsed_at, :utc_datetime
      timestamps(type: :utc_datetime)
    end

    create unique_index(:app_billing, [:customer_id])

    # Stripe's webhook events already handled, by id: a retry does nothing.
    create table(:stripe_events, primary_key: false) do
      add :id, :string, primary_key: true
      add :type, :string, null: false
      timestamps(type: :utc_datetime, updated_at: false)
    end

    # How much of each app's month each meter has been told (a cumulative
    # quantity: extra active users, call minutes), and where billing began
    # in that month (the quantity when the subscription started).
    create table(:billing_reports, primary_key: false) do
      add :app_id, references(:apps, type: :string, on_delete: :delete_all), primary_key: true
      add :month, :date, primary_key: true
      add :meter, :string, primary_key: true
      add :baseline, :bigint, null: false, default: 0
      add :reported, :bigint, null: false, default: 0
    end

    # Meter events to send to Stripe, written in the same transaction that
    # moves `billing_reports.reported`, then sent; `sent_at` once Stripe
    # took one. The identifier is Stripe's too, so a resend after a crash
    # is the same event.
    create table(:billing_meter_events, primary_key: false) do
      add :identifier, :string, primary_key: true
      add :app_id, references(:apps, type: :string, on_delete: :delete_all), null: false
      add :customer_id, :string, null: false
      add :event_name, :string, null: false
      add :value, :bigint, null: false
      add :timestamp, :utc_datetime, null: false
      add :sent_at, :utc_datetime
      add :attempts, :integer, null: false, default: 0
      add :last_error, :text
      timestamps(type: :utc_datetime, updated_at: false)
    end

    create index(:billing_meter_events, [:inserted_at], where: "sent_at IS NULL")
  end
end
