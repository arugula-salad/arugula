defmodule PresenceRelay.MeetTest do
  use PresenceRelay.DataCase, async: false
  import Plug.Conn
  import Phoenix.ConnTest

  @endpoint PresenceRelayWeb.Endpoint

  alias PresenceRelay.{Apps, Meet, Token}
  alias PresenceRelay.Accounts.Account
  alias PresenceRelayWeb.Socket

  setup do
    :ok = Apps.ensure_meet!()
    :ok
  end

  defp account(email, name \\ nil) do
    Repo.insert!(%Account{
      id: "acc_m#{System.unique_integer([:positive])}",
      email: email,
      name: name
    })
  end

  defp as(nil), do: build_conn()
  defp as(%Account{id: id}), do: build_conn() |> init_test_session(account_id: id)

  defp json_post(conn, path, body),
    do:
      conn
      |> put_req_header("content-type", "application/json")
      |> post(path, Jason.encode!(body))

  describe "routing" do
    test "a meet. host gets meet's page for every route; its API and sign-in are the relay's" do
      meet = fn path -> %{build_conn() | host: "meet.arugula.stream"} |> get(path) end

      for path <- ~w(/ /r/abcd-efgh /meet /meet/r/abcd-efgh) do
        assert response(meet.(path), 200) =~ "meet test index", path
      end

      assert response(meet.("/healthz"), 200) == "ok"
      assert response(meet.("/api/nope"), 404)
      assert %{"key" => "pk_live_" <> _} = meet.("/api/v1/meet/session") |> json_response(200)
    end

    test "under /meet on any host, meet's page; everything else is the demo's, as before" do
      for path <- ~w(/meet /meet/ /meet/r/abcd-efgh) do
        assert response(get(build_conn(), path), 200) =~ "meet test index", path
      end

      for path <- ~w(/ /r/standup /docs /hosted /meeting /r/meet) do
        assert response(get(build_conn(), path), 200) =~ "arugula.video test index", path
      end

      # A missing build file is missing, not meet's page in its place.
      assert response(get(build_conn(), "/meet/assets/nope.js"), 404)
    end
  end

  describe "the API" do
    test "the session: nobody or who, and meet's live key for guests" do
      assert %{"account" => nil, "key" => "pk_live_" <> _ = key, "sign_in" => %{"email" => true}} =
               as(nil) |> get("/api/v1/meet/session") |> json_response(200)

      ada = account("ada@example.com", "Ada")

      assert %{"account" => %{"id" => id, "email" => "ada@example.com"}, "key" => ^key} =
               as(ada) |> get("/api/v1/meet/session") |> json_response(200)

      assert id == ada.id
    end

    test "a guest makes a room with an unguessable slug; anyone reads it by its link" do
      %{"room" => room} =
        as(nil) |> json_post("/api/v1/meet/rooms", %{name: " Friday hang "}) |> json_response(201)

      assert %{"name" => "Friday hang", "mine" => false, "slug" => slug} = room
      assert slug =~ ~r/\A[0-9a-z]{4}-[0-9a-z]{4}\z/
      assert Meet.valid_slug?(slug)
      assert Repo.get!(Meet.Room, slug).creator_id == nil

      assert %{"room" => %{"slug" => ^slug, "name" => "Friday hang"}} =
               as(nil) |> get("/api/v1/meet/rooms/#{slug}") |> json_response(200)

      assert as(nil) |> get("/api/v1/meet/rooms/zzzz-zzzz") |> json_response(404)
      assert as(nil) |> get("/api/v1/meet/rooms/not-a-slug") |> json_response(404)

      for name <- ["", "   ", String.duplicate("x", 81), nil] do
        assert %{"error" => _} =
                 as(nil) |> json_post("/api/v1/meet/rooms", %{name: name}) |> json_response(400)
      end
    end

    test "a guest has no rooms list and gets no room token (guests go in by the key)" do
      {:ok, room} = Meet.create("Lobby", nil)
      assert as(nil) |> get("/api/v1/meet/rooms") |> json_response(401)

      assert as(nil)
             |> json_post("/api/v1/meet/rooms/#{room.slug}/token", %{})
             |> json_response(401)
    end

    test "writes must be JSON" do
      conn =
        as(nil)
        |> put_req_header("content-type", "application/x-www-form-urlencoded")
        |> post("/api/v1/meet/rooms", "name=Lobby")

      assert json_response(conn, 415)
    end

    test "rooms made and rooms joined: each list in its own order, made ones only once" do
      ada = account("ada@example.com", "Ada")
      ben = account("ben@example.com")

      %{"room" => %{"slug" => a1, "mine" => true}} =
        as(ada) |> json_post("/api/v1/meet/rooms", %{name: "Ada's"}) |> json_response(201)

      %{"room" => %{"slug" => b1}} =
        as(ben) |> json_post("/api/v1/meet/rooms", %{name: "Ben's"}) |> json_response(201)

      {:ok, g1} = Meet.create("A guest's", nil)

      # Ada joins Ben's, then the guest's, then her own.
      for slug <- [b1, g1.slug, a1] do
        assert %{"token" => _} =
                 as(ada)
                 |> json_post("/api/v1/meet/rooms/#{slug}/token", %{})
                 |> json_response(200)
      end

      # Joining Ben's again moves it to the top.
      Repo.update_all(Meet.Recent,
        set: [joined_at: DateTime.add(DateTime.utc_now(), -60) |> DateTime.truncate(:second)]
      )

      as(ada) |> json_post("/api/v1/meet/rooms/#{b1}/token", %{}) |> json_response(200)

      assert %{"made" => [%{"slug" => ^a1, "mine" => true}], "recent" => recent} =
               as(ada) |> get("/api/v1/meet/rooms") |> json_response(200)

      assert [%{"slug" => ^b1, "mine" => false}, %{"slug" => g}] = recent
      assert g == g1.slug

      assert %{"made" => [%{"slug" => ^b1}], "recent" => []} =
               as(ben) |> get("/api/v1/meet/rooms") |> json_response(200)
    end
  end

  describe "into the room" do
    test "a signed-in person's token is meet's, for the room's slug, as them" do
      ada = account("ada.l@example.com")
      {:ok, room} = Meet.create("Lobby", nil)

      %{"token" => token, "sub" => sub, "room" => slug} =
        as(ada)
        |> json_post("/api/v1/meet/rooms/#{room.slug}/token", %{
          name: "Ada L",
          look: %{character: 1, colour: 3}
        })
        |> json_response(200)

      assert sub == ada.id and slug == room.slug

      assert {:ok,
              %Token{
                app: "meet",
                room: ^slug,
                sub: ^sub,
                name: "Ada L",
                exp: exp,
                look: %{"character" => 1, "colour" => 3}
              }} = Token.verify(token)

      assert exp > System.system_time(:second) + 3600

      # The relay's socket takes it as it is (no change to the socket).
      assert {:ok, %{app: "meet", room: ^slug, sub: ^sub}} =
               Socket.admit(%{"token" => token}, nil)

      # Without a name: the address's first part.
      %{"token" => token} =
        as(ada) |> json_post("/api/v1/meet/rooms/#{slug}/token", %{}) |> json_response(200)

      assert {:ok, %Token{name: "ada.l"}} = Token.verify(token)

      assert as(ada)
             |> json_post("/api/v1/meet/rooms/zzzz-zzzz/token", %{})
             |> json_response(404)
    end

    test "a guest goes in by meet's key and a name, from meet's origin" do
      {:ok, room} = Meet.create("Lobby", nil)
      key = Meet.publishable_key()

      assert {:ok, %{app: "meet", room: slug, sub: "g_" <> _, name: "Cleo"}} =
               Socket.admit(
                 %{"key" => key, "room" => room.slug, "name" => "Cleo"},
                 "https://meet.arugula.stream"
               )

      assert slug == room.slug

      assert {:error, :origin} =
               Socket.admit(
                 %{"key" => key, "room" => room.slug, "name" => "Cleo"},
                 "https://elsewhere.example"
               )
    end

    test "a revoked key isn't handed out" do
      app = Repo.get!(Apps.App, "meet")
      old = Meet.publishable_key()
      :ok = Apps.revoke_key(app, old)
      assert Meet.publishable_key() == nil
      {:ok, _} = Apps.make_key(app, "live")
      assert "pk_live_" <> _ = new = Meet.publishable_key()
      assert new != old
    end
  end

  test "signing in from a room comes back to it, through a real form post" do
    {:ok, room} = Meet.create("Lobby", nil)
    back = "/meet/r/#{room.slug}"

    conn =
      as(nil) |> json_post("/api/v1/auth/email", %{email: "cleo@example.com", return_to: back})

    assert json_response(conn, 200)["sent"]

    assert_received {:email, %{text_body: body}}
    [_, token] = Regex.run(~r{/auth/email/([A-Za-z0-9_-]+)}, body)

    conn = conn |> recycle() |> get("/auth/email/#{token}")
    [_, csrf] = Regex.run(~r{name="_csrf_token" value="([^"]+)"}, html_response(conn, 200))

    # The button's form, encoded as a browser sends it, CSRF checked.
    conn =
      conn
      |> recycle()
      |> put_private(:plug_skip_csrf_protection, false)
      |> put_req_header("content-type", "application/x-www-form-urlencoded")
      |> post("/auth/email/#{token}", URI.encode_query(%{"_csrf_token" => csrf}))

    assert redirected_to(conn) == back

    assert %{"account" => %{"email" => "cleo@example.com"}} =
             conn |> recycle() |> get("/api/v1/meet/session") |> json_response(200)
  end

  test "without a database there's no meet" do
    Application.put_env(:presence_relay, :database, false)

    try do
      assert %{"error" => _} = as(nil) |> get("/api/v1/meet/session") |> json_response(503)
    after
      Application.put_env(:presence_relay, :database, true)
    end
  end
end
