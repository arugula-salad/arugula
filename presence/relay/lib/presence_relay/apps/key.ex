defmodule PresenceRelay.Apps.Key do
  @moduledoc "A publishable key (`pk_live_…`, `pk_test_…`): fine in a page; good only from the app's origins."
  use Ecto.Schema

  schema "app_keys" do
    belongs_to(:app, PresenceRelay.Apps.App, type: :string)
    field(:key, :string)
    field(:mode, :string)
    field(:revoked_at, :utc_datetime)
    timestamps(type: :utc_datetime, updated_at: false)
  end
end
