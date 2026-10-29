defmodule PresenceRelay.Billing.Report do
  @moduledoc """
  How much of an app's month (its first day) one meter has been told, as
  a cumulative quantity, and the quantity when billing began that month
  (`PresenceRelay.Billing.Reporter`).
  """
  use Ecto.Schema

  @primary_key false
  schema "billing_reports" do
    field(:app_id, :string, primary_key: true)
    field(:month, :date, primary_key: true)
    field(:meter, :string, primary_key: true)
    field(:baseline, :integer, default: 0)
    field(:reported, :integer, default: 0)
  end
end
