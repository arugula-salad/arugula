defmodule PresenceRelay.Repo.Migrations.CreateMessages do
  use Ecto.Migration

  def change do
    create table(:messages) do
      add :app, :string, null: false
      add :room, :string, null: false
      add :sub, :string, null: false
      add :name, :string, null: false
      add :text, :text, null: false
      timestamps(type: :utc_datetime_usec, updated_at: false)
    end

    create index(:messages, [:app, :room, :id])
  end
end
