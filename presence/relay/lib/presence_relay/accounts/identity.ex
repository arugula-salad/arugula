defmodule PresenceRelay.Accounts.Identity do
  @moduledoc "How an account signs in: `email` (the address) or `google` (Google's subject)."
  use Ecto.Schema

  schema "identities" do
    belongs_to(:account, PresenceRelay.Accounts.Account, type: :string)
    field(:provider, :string)
    field(:subject, :string)
    timestamps(type: :utc_datetime)
  end
end
