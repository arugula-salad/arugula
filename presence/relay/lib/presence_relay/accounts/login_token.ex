defmodule PresenceRelay.Accounts.LoginToken do
  @moduledoc "A magic link's token, by its SHA-256: works once, for a quarter of an hour."
  use Ecto.Schema

  schema "login_tokens" do
    field(:token_hash, :binary)
    field(:email, :string)
    field(:expires_at, :utc_datetime)
    field(:used_at, :utc_datetime)
    timestamps(type: :utc_datetime, updated_at: false)
  end
end
