defmodule PresenceRelay.Repo.Migrations.Usage do
  use Ecto.Migration

  def change do
    # An app's cap on monthly active users (#318 P9): null, none.
    alter table(:apps) do
      add :mau_cap, :integer
    end

    # Each app's day (UTC): how many people joined a room, and the seconds
    # people spent in calls (each leave's time rounded up to a second).
    create table(:usage_days, primary_key: false) do
      add :app_id, references(:apps, type: :string, on_delete: :delete_all), primary_key: true
      add :day, :date, primary_key: true
      add :active_users, :integer, null: false, default: 0
      add :call_seconds, :bigint, null: false, default: 0
    end

    # Who counted in an app's month (its first day), once each: the
    # monthly active users are its rows. `days` has bit d-1 set for each
    # day d of the month they joined on, so a person counts once a day
    # too, whichever node saw them first.
    create table(:usage_members, primary_key: false) do
      add :app_id, references(:apps, type: :string, on_delete: :delete_all), primary_key: true
      add :month, :date, primary_key: true
      add :member_key, :string, primary_key: true
      add :days, :integer, null: false, default: 0
    end
  end
end
