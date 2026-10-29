defmodule PresenceRelay.Usage.Day do
  @moduledoc "An app's day of use (UTC): people who joined a room, and seconds in calls."
  use Ecto.Schema

  @primary_key false
  schema "usage_days" do
    field(:app_id, :string, primary_key: true)
    field(:day, :date, primary_key: true)
    field(:active_users, :integer, default: 0)
    field(:call_seconds, :integer, default: 0)
  end
end
