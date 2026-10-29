defmodule PresenceRelay.Billing.MeterEvent do
  @moduledoc """
  A meter event for Stripe, kept until Stripe has it
  (`PresenceRelay.Billing.Reporter`). Its identifier is the one Stripe
  sees: `<app>:<yyyy-mm>:<meter>:<cumulative quantity>`.
  """
  use Ecto.Schema

  @primary_key {:identifier, :string, autogenerate: false}
  schema "billing_meter_events" do
    field(:app_id, :string)
    field(:customer_id, :string)
    field(:event_name, :string)
    field(:value, :integer)
    field(:timestamp, :utc_datetime)
    field(:sent_at, :utc_datetime)
    field(:attempts, :integer, default: 0)
    field(:last_error, :string)
    timestamps(type: :utc_datetime, updated_at: false)
  end
end
