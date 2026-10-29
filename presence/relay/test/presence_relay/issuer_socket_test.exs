defmodule PresenceRelay.IssuerSocketTest do
  use PresenceRelay.DataCase, async: false
  import Phoenix.ChannelTest
  import Phoenix.ConnTest, except: [connect: 2, connect: 3]
  import Plug.Conn

  @endpoint PresenceRelayWeb.Endpoint

  alias PresenceRelay.{Apps, Issuers}
  alias PresenceRelay.Accounts.Account
  alias PresenceRelay.TestIssuer, as: T
  alias PresenceRelayWeb.Socket

  setup_all do
    %{rsa: T.rsa("rsa-1")}
  end

  defp account(email) do
    Repo.insert!(%Account{id: "acc_t#{System.unique_integer([:positive])}", email: email})
  end

  setup ctx do
    srv = T.start([ctx.rsa])
    owner = account("o#{System.unique_integer([:positive])}@example.com")
    {:ok, app, _} = Apps.create(owner, %{"name" => "Acme", "guests" => true})
    {:ok, _} = Apps.add_origin(app, "https://acme.com")

    {:ok, _} =
      Issuers.put(app, %{"provider" => "oidc", "issuer" => srv.url, "audience" => "acme"})

    [live] = app.keys
    %{srv: srv, app: app, key: live.key, owner: owner}
  end

  defp tok(ctx, more \\ %{}),
    do:
      T.sign(
        ctx.rsa,
        Map.merge(
          %{"iss" => ctx.srv.url, "aud" => "acme", "sub" => "user_1", "name" => "Ada"},
          more
        )
      )

  defp p(ctx, more), do: Map.merge(%{"key" => ctx.key, "room" => "lobby"}, more)

  test "key + auth admits a verified member, who joins the room as anyone would", ctx do
    assert {:ok, %{sub: "oidc-" <> _ = sub, name: "Ada", room: "lobby", app: app}} =
             Socket.admit(p(ctx, %{"auth" => tok(ctx), "look" => "1:2"}), "https://acme.com")

    assert app == ctx.app.id
    assert String.ends_with?(sub, ":user_1")

    assert {:ok, socket} =
             connect(Socket, p(ctx, %{"auth" => tok(ctx)}),
               connect_info: %{origin: "https://acme.com"}
             )

    assert {:ok, %{you: ^sub}, _} = subscribe_and_join(socket, "room:lobby", %{})
  end

  test "the key's origins hold for members too", ctx do
    assert {:error, :origin} = Socket.admit(p(ctx, %{"auth" => tok(ctx)}), "https://evil.com")
    assert {:error, :origin} = Socket.admit(p(ctx, %{"auth" => tok(ctx)}), nil)
  end

  test "a bad token is refused, never let in as a guest instead", ctx do
    bad = tok(ctx, %{"aud" => "someone-else"})

    assert {:error, :wrong_audience} =
             Socket.admit(p(ctx, %{"auth" => bad, "name" => "Ada"}), "https://acme.com")

    assert {:error, :expired} =
             Socket.admit(p(ctx, %{"auth" => tok(ctx, %{"exp" => 1})}), "https://acme.com")
  end

  test "guests still come by name where the app allows them", ctx do
    assert {:ok, %{sub: "g_" <> _, name: "Bo"}} =
             Socket.admit(p(ctx, %{"name" => "Bo"}), "https://acme.com")

    {:ok, _} = Apps.update(ctx.app, %{"guests" => false})
    assert {:error, :no_guests} = Socket.admit(p(ctx, %{"name" => "Bo"}), "https://acme.com")
    assert {:ok, _} = Socket.admit(p(ctx, %{"auth" => tok(ctx)}), "https://acme.com")
  end

  test "a token sent with another app's key is refused", ctx do
    other = account("b#{System.unique_integer([:positive])}@example.com")
    {:ok, b, _} = Apps.create(other, %{"name" => "Globex"})
    {:ok, _} = Apps.add_origin(b, "https://acme.com")
    [bkey] = b.keys
    pb = %{"key" => bkey.key, "room" => "lobby", "auth" => tok(ctx)}

    # No issuer at all.
    assert {:error, :no_issuer} = Socket.admit(pb, "https://acme.com")

    # The same provider, but its own audience.
    {:ok, _} =
      Issuers.put(b, %{"provider" => "oidc", "issuer" => ctx.srv.url, "audience" => "globex"})

    assert {:error, :wrong_audience} = Socket.admit(pb, "https://acme.com")

    # Another provider altogether.
    srv2 = T.start([ctx.rsa])
    {:ok, _} = Issuers.put(b, %{"provider" => "oidc", "issuer" => srv2.url, "audience" => "acme"})
    assert {:error, :wrong_issuer} = Socket.admit(pb, "https://acme.com")
  end

  test "the app's room rule decides members' rooms", ctx do
    {:ok, _} =
      Issuers.put(ctx.app, %{
        "provider" => "oidc",
        "issuer" => ctx.srv.url,
        "audience" => "acme",
        "room_rule" => %{"claim" => "org_id", "match" => "prefix"}
      })

    auth = tok(ctx, %{"org_id" => "acme"})

    assert {:ok, _} =
             Socket.admit(p(ctx, %{"auth" => auth, "room" => "acme:standup"}), "https://acme.com")

    assert {:error, :room_rule} =
             Socket.admit(
               p(ctx, %{"auth" => auth, "room" => "globex:standup"}),
               "https://acme.com"
             )
  end

  describe "the apps API" do
    defp signed_in(account), do: build_conn() |> init_test_session(account_id: account.id)

    defp put_json(conn, path, body),
      do:
        conn
        |> put_req_header("content-type", "application/json")
        |> put(path, Jason.encode!(body))

    test "the owner gets, sets and takes away the issuer; others can't", ctx do
      path = "/api/v1/apps/#{ctx.app.id}/issuer"
      conn = signed_in(ctx.owner)

      assert %{"issuer" => %{"provider" => "oidc", "audience" => "acme", "discovery" => _}} =
               conn |> get(path) |> json_response(200)

      body = %{
        provider: "clerk",
        issuer: ctx.srv.url <> "/",
        room_rule: %{claim: "o.id", match: "prefix"}
      }

      assert %{"issuer" => i} = conn |> put_json(path, body) |> json_response(200)

      assert %{
               "provider" => "clerk",
               "issuer" => issuer,
               "iss" => issuer,
               "audience" => nil,
               "name_claim" => "name",
               "room_rule" => %{"claim" => "o.id", "match" => "prefix"}
             } = i

      assert issuer == ctx.srv.url
      assert i["jwks"] == ctx.srv.url <> "/.well-known/jwks.json"
      assert %{"issuer" => %{"provider" => "clerk"}} = conn |> get(path) |> json_response(200)

      assert %{"error" => "auth0 needs an audience"} =
               conn
               |> put_json(path, %{provider: "auth0", issuer: "acme.auth0.com"})
               |> json_response(400)

      stranger = signed_in(account("s#{System.unique_integer([:positive])}@example.com"))
      assert stranger |> get(path) |> json_response(404)
      assert stranger |> put_json(path, body) |> json_response(404)
      assert build_conn() |> get(path) |> json_response(401)

      assert %{"issuer" => nil} = conn |> delete(path) |> json_response(200)
      assert %{"issuer" => nil} = conn |> get(path) |> json_response(200)

      assert {:error, :no_issuer} =
               Socket.admit(p(ctx, %{"auth" => tok(ctx)}), "https://acme.com")
    end
  end
end
