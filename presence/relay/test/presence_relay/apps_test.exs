defmodule PresenceRelay.AppsTest do
  use PresenceRelay.DataCase, async: false

  alias PresenceRelay.{Accounts, Apps, Token}
  alias PresenceRelay.Accounts.Account

  defp owner(email \\ "owner@example.com") do
    Repo.insert!(%Account{
      id: "acc_" <> Integer.to_string(System.unique_integer([:positive])),
      email: email
    })
  end

  test "an app has a live key, its secret signs room tokens the relay accepts, and rotating it voids the old one" do
    o = owner()
    {:ok, app, secret} = Apps.create(o, %{"name" => "Acme"})
    assert [%{mode: "live", key: "pk_live_" <> _}] = app.keys
    assert Apps.secret(app.id) == secret

    claims = %{
      "room" => "lobby",
      "sub" => "u1",
      "name" => "Ada",
      "exp" => System.system_time(:second) + 600
    }

    token = Token.sign(app.id, claims)
    assert {:ok, %{app: id}} = Token.verify(token)
    assert id == app.id

    {:ok, new} = Apps.rotate_secret(app)
    refute new == secret
    assert {:error, :bad_signature} = Token.verify(token)
    assert {:ok, _} = Token.verify(Token.sign(app.id, claims))
  end

  test "the secret is kept encrypted, not as itself" do
    {:ok, app, secret} = Apps.create(owner(), %{"name" => "Acme"})
    enc = Repo.get!(Apps.App, app.id).secret_enc
    refute enc =~ secret
  end

  test "apps are their owner's" do
    a = owner("a@example.com")
    b = owner("b@example.com")
    {:ok, app, _} = Apps.create(a, %{"name" => "Mine"})
    assert [%{id: id}] = Apps.list(a)
    assert id == app.id
    assert Apps.list(b) == []
    assert Apps.get(b, app.id) == nil
    assert {:error, _} = Apps.create(a, %{"name" => ""})
  end

  test "origins are normalized and checked; a test key also works from localhost" do
    {:ok, app, _} = Apps.create(owner(), %{"name" => "Acme"})
    assert {:ok, "https://acme.com"} = Apps.add_origin(app, "https://ACME.com/")
    assert {:ok, "https://*.acme.dev"} = Apps.add_origin(app, "https://*.acme.dev")
    assert {:ok, "http://localhost:3000"} = Apps.add_origin(app, "http://localhost:3000")
    assert {:error, _} = Apps.add_origin(app, "ftp://acme.com")
    assert {:error, _} = Apps.add_origin(app, "https://acme.com/path")

    allowed = Apps.get(app.owner_id |> Accounts.get(), app.id).origins |> Enum.map(& &1.origin)
    assert Apps.origin_allowed?(allowed, "live", "https://acme.com")
    assert Apps.origin_allowed?(allowed, "live", "https://app.acme.dev")
    refute Apps.origin_allowed?(allowed, "live", "https://acme.dev.evil.com")
    refute Apps.origin_allowed?(allowed, "live", "http://acme.com")
    refute Apps.origin_allowed?(allowed, "live", "https://evil.com")
    refute Apps.origin_allowed?(allowed, "live", "http://localhost:5173")
    assert Apps.origin_allowed?(allowed, "test", "http://localhost:5173")
    refute Apps.origin_allowed?(allowed, "live", nil)

    {:ok, _} = Apps.remove_origin(app, "https://acme.com")
    allowed = Apps.get(Accounts.get(app.owner_id), app.id).origins |> Enum.map(& &1.origin)
    refute Apps.origin_allowed?(allowed, "live", "https://acme.com")
  end

  test "keys: made, found with their app and origins, revoked" do
    {:ok, app, _} = Apps.create(owner(), %{"name" => "Acme"})
    {:ok, key} = Apps.make_key(app, "test")
    assert %{app: %{id: id, origins: []}, mode: "test"} = Apps.by_key(key.key)
    assert id == app.id
    assert :ok = Apps.revoke_key(app, key.key)
    assert Apps.by_key(key.key) == nil
    assert {:error, :not_found} = Apps.revoke_key(app, key.key)
    assert Apps.by_key("pk_live_nope") == nil
    assert {:error, _} = Apps.make_key(app, "prod")
  end

  test "meet is seeded once, owned by the system account, guests allowed" do
    :ok = Apps.ensure_meet!()
    :ok = Apps.ensure_meet!()
    app = Repo.get!(Apps.App, "meet") |> Repo.preload(:origins)
    assert app.owner_id == "acc_system"
    assert app.guests
    assert "https://meet.arugula.stream" in Enum.map(app.origins, & &1.origin)
    assert is_binary(Apps.secret("meet"))
  end

  test "static apps still come first" do
    assert Apps.secret("acme") == "acme-test-secret"
    assert Apps.secret("app_unknown") == nil
  end
end
