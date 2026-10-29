defmodule PresenceRelay.LimitsTest do
  use PresenceRelay.DataCase, async: false
  import Plug.Conn
  import Phoenix.ConnTest
  import Swoosh.TestAssertions
  import PresenceRelay.RelayCase, only: [client: 4, client_with: 4, room: 0, token: 1, token: 2]

  alias PresenceRelay.{Apps, Limits}
  alias PresenceRelayWeb.{ClientIP, Socket}

  @endpoint PresenceRelayWeb.Endpoint

  # The limits as the test sets them, for this test only.
  defp limits(changes) do
    before = Application.get_env(:presence_relay, :rate_limits)
    Application.put_env(:presence_relay, :rate_limits, Map.merge(before, changes))
    Limits.reset()
    on_exit(fn -> Application.put_env(:presence_relay, :rate_limits, before) end)
  end

  defp put_env(key, value) do
    before = Application.get_env(:presence_relay, key)
    Application.put_env(:presence_relay, key, value)
    on_exit(fn -> Application.put_env(:presence_relay, key, before) end)
  end

  defp from_ip(ip), do: %{build_conn() | remote_ip: ip}

  defp json_post(conn, path, body),
    do:
      conn
      |> put_req_header("content-type", "application/json")
      |> post(path, Jason.encode!(body))

  describe "counting" do
    test "a key gets its hits a window, then a retry-after; another key has its own" do
      limits(%{demo_token: {3, 60}})

      for _ <- 1..3, do: assert(:ok = Limits.hit(:demo_token, "198.51.100.1"))
      assert {:error, retry} = Limits.hit(:demo_token, "198.51.100.1")
      assert retry in 1..60
      assert :ok = Limits.hit(:demo_token, "198.51.100.2")
    end

    test "a limit that's off, or a client without an IP, isn't limited" do
      limits(%{demo_token: nil, meet_room: {1, 60}})

      for _ <- 1..10 do
        assert :ok = Limits.hit(:demo_token, "198.51.100.1")
        assert :ok = Limits.hit(:meet_room, nil)
      end
    end

    test "RATE_LIMITS changes the limits it names and keeps the rest" do
      base = %{socket_connect: {120, 60}, demo_token: {120, 3600}, meet_room: {20, 3600}}

      assert Limits.parse("socket_connect=600/60, demo_token=off", base) ==
               %{socket_connect: {600, 60}, demo_token: nil, meet_room: {20, 3600}}

      for bad <- ["nope=1/1", "socket_connect=1", "socket_connect=x/60", "meet_room=5/0"] do
        assert_raise RuntimeError, fn -> Limits.parse(bad, base) end
      end
    end
  end

  describe "the client's IP" do
    test "off Fly, Fly-Client-IP is anyone's to send: the peer address counts" do
      put_env(:trust_fly_client_ip, false)
      conn = from_ip({10, 1, 2, 3}) |> put_req_header("fly-client-ip", "203.0.113.7")
      assert ClientIP.of(conn) == "10.1.2.3"
    end

    test "behind Fly's proxy, Fly-Client-IP is the client's; a bad one falls back to the peer" do
      put_env(:trust_fly_client_ip, true)
      conn = fn h -> from_ip({172, 16, 0, 1}) |> put_req_header("fly-client-ip", h) end

      assert ClientIP.of(conn.("203.0.113.7")) == "203.0.113.7"
      assert ClientIP.of(conn.(" 2001:db8::1 ")) == "2001:db8::1"
      assert ClientIP.of(conn.("203.0.113.7, 10.0.0.1")) == "172.16.0.1"
      assert ClientIP.of(conn.("not an ip")) == "172.16.0.1"
      assert ClientIP.of(from_ip({172, 16, 0, 1})) == "172.16.0.1"
    end

    test "on Fly, two clients behind the proxy count apart; off Fly, they'd be one" do
      limits(%{demo_token: {1, 60}})
      tok = fn ip -> from_ip({172, 16, 0, 1}) |> put_req_header("fly-client-ip", ip) end
      body = %{"room" => "standup", "name" => "Jake"}

      put_env(:trust_fly_client_ip, true)
      assert json_response(post(tok.("203.0.113.1"), "/api/demo/token", body), 200)
      assert json_response(post(tok.("203.0.113.2"), "/api/demo/token", body), 200)
      assert json_response(post(tok.("203.0.113.1"), "/api/demo/token", body), 429)

      Application.put_env(:presence_relay, :trust_fly_client_ip, false)
      Limits.reset()
      assert json_response(post(tok.("203.0.113.3"), "/api/demo/token", body), 200)
      assert json_response(post(tok.("203.0.113.4"), "/api/demo/token", body), 429)
    end
  end

  describe "over a limit on HTTP" do
    test "demo tokens: a 429 with Retry-After and when to try again" do
      limits(%{demo_token: {2, 60}})

      ask = fn ->
        post(from_ip({10, 0, 0, 1}), "/api/demo/token", %{"room" => "r", "name" => "J"})
      end

      assert json_response(ask.(), 200)
      assert json_response(ask.(), 200)
      conn = ask.()
      assert %{"error" => "too many requests; try again in " <> _} = json_response(conn, 429)
      assert [retry] = get_resp_header(conn, "retry-after")
      assert String.to_integer(retry) in 1..60
    end

    test "meet rooms: so many an hour per IP, for guests too" do
      :ok = Apps.ensure_meet!()
      limits(%{meet_room: {2, 3600}})
      make = fn ip -> json_post(from_ip(ip), "/api/v1/meet/rooms", %{name: "Standup"}) end

      assert json_response(make.({10, 0, 0, 2}), 201)
      assert json_response(make.({10, 0, 0, 2}), 201)
      assert json_response(make.({10, 0, 0, 2}), 429)
      assert json_response(make.({10, 0, 0, 3}), 201)
    end

    test "JSON API writes are limited; reads aren't" do
      limits(%{api_write: {2, 60}})
      ip = {10, 0, 0, 4}

      assert json_response(json_post(from_ip(ip), "/api/v1/auth/logout", %{}), 200)
      assert json_response(json_post(from_ip(ip), "/api/v1/auth/logout", %{}), 200)
      assert json_response(json_post(from_ip(ip), "/api/v1/auth/logout", %{}), 429)
      assert json_response(get(from_ip(ip), "/api/v1/auth/providers"), 200)
    end

    test "sign-in links: a 429 per IP; an address over its own says sent and sends nothing" do
      limits(%{email_ip: {3, 3600}, email_address: {1, 3600}})
      ask = fn ip, email -> json_post(from_ip(ip), "/api/v1/auth/email", %{email: email}) end

      assert %{"sent" => true} = json_response(ask.({10, 0, 0, 5}, "ada@example.com"), 200)
      assert_email_sent(to: "ada@example.com")

      # The same address again, from anywhere: the same answer, no mail.
      assert %{"sent" => true} = json_response(ask.({10, 0, 0, 6}, "ADA@example.com"), 200)
      refute_email_sent()

      assert %{"sent" => true} = json_response(ask.({10, 0, 0, 5}, "bea@example.com"), 200)
      assert_email_sent(to: "bea@example.com")
      assert json_response(ask.({10, 0, 0, 5}, "cy@example.com"), 200)
      conn = ask.({10, 0, 0, 5}, "dee@example.com")
      assert json_response(conn, 429)
      assert get_resp_header(conn, "retry-after") != []
    end
  end

  describe "over a limit on the socket" do
    test "connects per IP: refused as rate limited, which the upgrade answers 429" do
      limits(%{socket_connect: {2, 60}})
      info = %{peer_data: %{address: {10, 0, 0, 7}, port: 1, ssl_cert: nil}}
      params = %{"token" => token(%{"room" => room()})}

      assert {:ok, _} = Socket.connect(params, %Phoenix.Socket{}, info)
      assert {:ok, _} = Socket.connect(params, %Phoenix.Socket{}, info)
      assert {:error, {:rate_limited, retry}} = Socket.connect(params, %Phoenix.Socket{}, info)

      conn = Socket.handle_error(build_conn(), {:rate_limited, retry})
      assert conn.status == 429
      assert get_resp_header(conn, "retry-after") == [Integer.to_string(retry)]
      assert Socket.handle_error(build_conn(), :whatever).status == 403
    end

    test "an app's connections at once are capped across its rooms" do
      # Other tests' clients may still be about: two more than there are.
      put_env(:app_connections, %{"acme" => PresenceRelay.Rooms.app_connections("acme") + 2})
      {r1, r2} = {room(), room()}
      n = PresenceRelay.Rooms.app_connections("other")

      assert {:ok, _, _} = client(:a, r1, "u_a", [])
      assert {:ok, _, _} = client(:b, r2, "u_b", [])

      assert {:error, %{reason: "this app has too many connections"}} =
               client(:c, r1, "u_c", [])

      # Another app's aren't counted.
      assert {:ok, _, _} =
               client_with(node(), :d, r1, %{"token" => token(%{"room" => r1}, "other")})

      assert PresenceRelay.Rooms.app_connections("other") == n + 1
    end
  end
end
