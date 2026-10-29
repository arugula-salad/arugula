defmodule PresenceRelay.Apps.App do
  @moduledoc """
  A host app in the database: its id (what room tokens' `app` claim
  names), its owner, its HS256 secret (encrypted: see `PresenceRelay.Apps`),
  whether guests may join its rooms, its publishable keys and origins, and
  its sign-in provider (`PresenceRelay.Issuers`).
  """
  use Ecto.Schema

  @primary_key {:id, :string, autogenerate: false}
  schema "apps" do
    belongs_to(:owner, PresenceRelay.Accounts.Account, type: :string)
    field(:name, :string)
    field(:secret_enc, :binary)
    field(:guests, :boolean, default: false)
    has_many(:keys, PresenceRelay.Apps.Key)
    has_many(:origins, PresenceRelay.Apps.Origin)
    # Its sign-in provider, for members by `auth` (#318 P4), if any.
    has_one(:issuer, PresenceRelay.Issuers.Issuer)
    timestamps(type: :utc_datetime)
  end
end
