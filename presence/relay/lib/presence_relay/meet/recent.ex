defmodule PresenceRelay.Meet.Recent do
  @moduledoc "An account was in a meet room: when it last joined."
  use Ecto.Schema

  @primary_key false
  schema "meet_recent" do
    field(:account_id, :string, primary_key: true)
    field(:slug, :string, primary_key: true)
    field(:joined_at, :utc_datetime)
  end
end
