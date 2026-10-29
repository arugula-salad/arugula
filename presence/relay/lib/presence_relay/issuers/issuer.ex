defmodule PresenceRelay.Issuers.Issuer do
  @moduledoc """
  An app's sign-in provider (`PresenceRelay.Issuers`): a preset and its
  settings, and an optional room rule. One per app, keyed by the app.
  """
  use Ecto.Schema

  @primary_key false
  schema "app_issuers" do
    belongs_to(:app, PresenceRelay.Apps.App, type: :string, primary_key: true)
    field(:provider, :string)
    field(:issuer, :string)
    field(:project, :string)
    field(:audience, :string)
    field(:name_claim, :string)
    field(:room_claim, :string)
    field(:room_match, :string)
    timestamps(type: :utc_datetime)
  end
end
