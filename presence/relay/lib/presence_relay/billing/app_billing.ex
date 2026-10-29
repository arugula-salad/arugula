defmodule PresenceRelay.Billing.AppBilling do
  @moduledoc """
  An app's place in Stripe (`PresenceRelay.Billing`): its customer, and
  its subscription's state as the webhooks last told it.
  """
  use Ecto.Schema

  @primary_key {:app_id, :string, autogenerate: false}
  schema "app_billing" do
    field(:customer_id, :string)
    field(:subscription_id, :string)
    field(:status, :string)
    field(:cancel_at_period_end, :boolean, default: false)
    field(:current_period_end, :utc_datetime)
    field(:lapsed_at, :utc_datetime)
    timestamps(type: :utc_datetime)
  end
end
