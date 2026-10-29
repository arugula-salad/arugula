defmodule PresenceRelay.Meet.Room do
  @moduledoc "A meet room: its slug (the link's and the socket's room), its name, and who made it."
  use Ecto.Schema

  @primary_key {:slug, :string, autogenerate: false}
  schema "meet_rooms" do
    field(:name, :string)
    belongs_to(:creator, PresenceRelay.Accounts.Account, type: :string)
    timestamps(type: :utc_datetime, inserted_at: :created_at, updated_at: false)
  end
end
