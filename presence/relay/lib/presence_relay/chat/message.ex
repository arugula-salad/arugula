defmodule PresenceRelay.Chat.Message do
  @moduledoc "A kept message: who said what in which app's room, and when."
  use Ecto.Schema

  schema "messages" do
    field(:app, :string)
    field(:room, :string)
    field(:sub, :string)
    field(:name, :string)
    field(:text, :string)
    timestamps(type: :utc_datetime_usec, updated_at: false)
  end
end
