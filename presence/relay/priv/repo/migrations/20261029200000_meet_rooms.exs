defmodule PresenceRelay.Repo.Migrations.MeetRooms do
  use Ecto.Migration

  def change do
    # meet.arugula.stream's rooms (#318 P6): a short unguessable slug (the
    # link, and the room on the socket), a name, and who made it (null for
    # a guest).
    create table(:meet_rooms, primary_key: false) do
      add :slug, :string, primary_key: true
      add :name, :string, null: false
      add :creator_id, references(:accounts, type: :string, on_delete: :nilify_all)
      timestamps(type: :utc_datetime, inserted_at: :created_at, updated_at: false)
    end

    create index(:meet_rooms, [:creator_id, :created_at])

    # The rooms each account has been in lately, newest first on its home.
    create table(:meet_recent, primary_key: false) do
      add :account_id, references(:accounts, type: :string, on_delete: :delete_all),
        primary_key: true

      add :slug, references(:meet_rooms, column: :slug, type: :string, on_delete: :delete_all),
        primary_key: true

      add :joined_at, :utc_datetime, null: false
    end

    create index(:meet_recent, [:account_id, :joined_at])
  end
end
