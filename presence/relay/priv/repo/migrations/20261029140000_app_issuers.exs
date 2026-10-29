defmodule PresenceRelay.Repo.Migrations.AppIssuers do
  use Ecto.Migration

  def change do
    # An app's sign-in provider (#318 P4), at most one: the relay verifies
    # its end users' own tokens against the provider's JWKS, so a page with
    # a publishable key can put signed-in people in rooms with no server.
    create table(:app_issuers, primary_key: false) do
      add :app_id, references(:apps, type: :string, on_delete: :delete_all), primary_key: true
      # clerk, auth0, firebase, supabase or oidc.
      add :provider, :string, null: false
      # The provider's URL (Clerk's Frontend API, the Auth0 tenant, the
      # Supabase project, an OIDC issuer); Firebase names a project instead.
      add :issuer, :string
      add :project, :string
      add :audience, :string
      # Where the member's name is in the token (`a.b` walks into maps).
      add :name_claim, :string, null: false
      # A room rule, or none: the claim, and `prefix` or `contains`.
      add :room_claim, :string
      add :room_match, :string
      timestamps(type: :utc_datetime)
    end
  end
end
