defmodule PresenceRelay.Usage.Member do
  @moduledoc """
  One person counted in an app's month (`month` is its first day), by
  their metering key (`PresenceRelay.Usage`); `days` has bit d-1 set for
  each day d they joined on.
  """
  use Ecto.Schema

  @primary_key false
  schema "usage_members" do
    field(:app_id, :string, primary_key: true)
    field(:month, :date, primary_key: true)
    field(:member_key, :string, primary_key: true)
    field(:days, :integer, default: 0)
  end
end
