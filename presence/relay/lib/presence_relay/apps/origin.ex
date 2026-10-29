defmodule PresenceRelay.Apps.Origin do
  @moduledoc "An origin a publishable key works from: `https://example.com`, `https://*.example.com`."
  use Ecto.Schema

  schema "app_origins" do
    belongs_to(:app, PresenceRelay.Apps.App, type: :string)
    field(:origin, :string)
    timestamps(type: :utc_datetime, updated_at: false)
  end
end
