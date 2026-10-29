defmodule PresenceRelay.Accounts.Account do
  @moduledoc "Someone who signs in to the dashboard or meet (or the system account)."
  use Ecto.Schema

  @primary_key {:id, :string, autogenerate: false}
  schema "accounts" do
    field(:email, :string)
    field(:name, :string)
    field(:kind, :string, default: "user")
    has_many(:identities, PresenceRelay.Accounts.Identity)
    timestamps(type: :utc_datetime)
  end
end
