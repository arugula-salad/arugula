defmodule PresenceRelay.KeySocketTest do
  use PresenceRelay.DataCase, async: false
  import Phoenix.ChannelTest
  import Plug.Conn
  import Phoenix.ConnTest, only: [build_conn: 2]

  @endpoint PresenceRelayWeb.Endpoint

  alias PresenceRelay.Apps
  alias PresenceRelay.Accounts.Account
  alias PresenceRelayWeb.{Origin, Socket}

  setup do
    owner =
      Repo.insert!(%Account{
        id: "acc_t#{System.unique_integer([:positive])}",
        email: "o@example.com"
      })

    {:ok, app, _} = Apps.create(owner, %{"name" => "Acme", "guests" => true})
    {:ok, _} = Apps.add_origin(app, "https://acme.com")
    [live] = app.keys
    {:ok, test_key} = Apps.make_key(app, "test")
    %{app: app, key: live.key, test_key: test_key.key, owner: owner}
  end

  test "a key from an allowed origin admits a guest by name, with a sub of its own", %{
    key: key,
    app: app
  } do
    assert {:ok, a} =
             Socket.admit(
               %{"key" => key, "room" => "lobby", "name" => "Ada", "look" => "2:3"},
               "https://acme.com"
             )

    assert %{
             app: id,
             room: "lobby",
             name: "Ada",
             sub: "g_" <> _,
             look: %{"character" => 2, "colour" => 3}
           } = a

    assert id == app.id
  end

  test "a key from another origin, or none, is refused; a test key works from localhost", ctx do
    p = %{"key" => ctx.key, "room" => "lobby", "name" => "Ada"}
    assert {:error, :origin} = Socket.admit(p, "https://evil.com")
    assert {:error, :origin} = Socket.admit(p, nil)
    assert {:error, :origin} = Socket.admit(p, "http://localhost:3000")
    assert {:ok, _} = Socket.admit(%{p | "key" => ctx.test_key}, "http://localhost:3000")
  end

  test "revoked keys, guests turned off, bad rooms and names, and no identity are refused", ctx do
    p = %{"key" => ctx.key, "room" => "lobby", "name" => "Ada"}
    assert {:error, :bad_room} = Socket.admit(%{p | "room" => "no spaces"}, "https://acme.com")
    assert {:error, :bad_name} = Socket.admit(%{p | "name" => ""}, "https://acme.com")
    assert {:error, :no_identity} = Socket.admit(Map.delete(p, "name"), "https://acme.com")
    {:ok, _} = Apps.update(ctx.app, %{"guests" => false})
    assert {:error, :no_guests} = Socket.admit(p, "https://acme.com")
    :ok = Apps.revoke_key(ctx.app, ctx.key)
    assert {:error, :unknown_key} = Socket.admit(p, "https://acme.com")
    assert {:error, :no_credentials} = Socket.admit(%{}, "https://acme.com")
  end

  test "a guest's socket joins its room as a token's would", %{key: key} do
    assert {:ok, socket} =
             connect(Socket, %{"key" => key, "room" => "lobby", "name" => "Ada"},
               connect_info: %{origin: "https://acme.com"}
             )

    assert {:ok, %{you: "g_" <> _}, _} = subscribe_and_join(socket, "room:lobby", %{})

    assert :error =
             connect(Socket, %{"key" => key, "room" => "lobby", "name" => "Ada"},
               connect_info: %{origin: "https://evil.com"}
             )
  end

  test "the origin hook notes the origin for connect, which takes it once" do
    assert Origin.note(URI.parse("https://acme.com")) == true
    assert Origin.take() == "https://acme.com"
    assert Origin.take() == nil
    Origin.note(URI.parse("http://localhost:3000"))
    assert Origin.take() == "http://localhost:3000"
  end

  test "through the WebSocket transport: the Origin header decides a key's socket", %{key: key} do
    ws = fn origin ->
      build_conn(:get, "/socket/websocket?vsn=2.0.0&key=#{key}&room=lobby&name=Ada")
      |> put_req_header("origin", origin)
      |> put_req_header("connection", "upgrade")
      |> put_req_header("upgrade", "websocket")
      |> put_req_header("sec-websocket-version", "13")
      |> put_req_header("sec-websocket-key", Base.encode64(:crypto.strong_rand_bytes(16)))
      |> @endpoint.call(@endpoint.init([]))
    end

    assert ws.("https://evil.com").status == 403
    refute ws.("https://acme.com").status == 403
  end
end
