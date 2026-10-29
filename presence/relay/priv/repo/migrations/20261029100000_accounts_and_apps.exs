defmodule PresenceRelay.Repo.Migrations.AccountsAndApps do
  use Ecto.Migration

  def change do
    execute "CREATE EXTENSION IF NOT EXISTS citext", ""

    # People who sign in (and the system account that owns `meet`).
    create table(:accounts, primary_key: false) do
      add :id, :string, primary_key: true
      add :email, :citext
      add :name, :string
      add :kind, :string, null: false, default: "user"
      timestamps(type: :utc_datetime)
    end

    create unique_index(:accounts, [:email])

    # How an account signs in: an email address, a Google subject.
    create table(:identities) do
      add :account_id, references(:accounts, type: :string, on_delete: :delete_all), null: false
      add :provider, :string, null: false
      add :subject, :string, null: false
      timestamps(type: :utc_datetime)
    end

    create unique_index(:identities, [:provider, :subject])
    create index(:identities, [:account_id])

    # Magic links: only the token's hash is kept; each works once.
    create table(:login_tokens) do
      add :token_hash, :binary, null: false
      add :email, :citext, null: false
      add :expires_at, :utc_datetime, null: false
      add :used_at, :utc_datetime
      timestamps(type: :utc_datetime, updated_at: false)
    end

    create unique_index(:login_tokens, [:token_hash])

    # A host app: what room tokens and publishable keys name.
    create table(:apps, primary_key: false) do
      add :id, :string, primary_key: true
      add :owner_id, references(:accounts, type: :string, on_delete: :restrict), null: false
      add :name, :string, null: false
      # The HS256 secret for server-signed room tokens, encrypted with a key
      # derived from SECRET_KEY_BASE (it has to come back: HS256 is symmetric).
      add :secret_enc, :binary, null: false
      add :guests, :boolean, null: false, default: false
      timestamps(type: :utc_datetime)
    end

    create index(:apps, [:owner_id])

    # Publishable keys: safe in a page, good only from the app's origins.
    create table(:app_keys) do
      add :app_id, references(:apps, type: :string, on_delete: :delete_all), null: false
      add :key, :string, null: false
      add :mode, :string, null: false
      add :revoked_at, :utc_datetime
      timestamps(type: :utc_datetime, updated_at: false)
    end

    create unique_index(:app_keys, [:key])
    create index(:app_keys, [:app_id])

    create table(:app_origins) do
      add :app_id, references(:apps, type: :string, on_delete: :delete_all), null: false
      add :origin, :string, null: false
      timestamps(type: :utc_datetime, updated_at: false)
    end

    create unique_index(:app_origins, [:app_id, :origin])
  end
end
