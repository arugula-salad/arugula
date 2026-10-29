defmodule PresenceRelay.HttpTest do
  use ExUnit.Case, async: false
  import Plug.Conn
  import Phoenix.ConnTest

  @endpoint PresenceRelayWeb.Endpoint

  test "a demo relay hands out demo tokens that admit a socket, for a good room and name" do
    conn = post(build_conn(), "/api/demo/token", %{"room" => "standup", "name" => "Jake"})
    assert %{"token" => token, "room" => "standup", "sub" => "u_" <> _} = json_response(conn, 200)

    assert {:ok, %{app: "demo", room: "standup", name: "Jake"}} =
             PresenceRelay.Token.verify(token)

    assert json_response(
             post(build_conn(), "/api/demo/token", %{"room" => "no spaces", "name" => "J"}),
             400
           )

    assert json_response(
             post(build_conn(), "/api/demo/token", %{"room" => "ok", "name" => ""}),
             400
           )
  end

  test "without the demo, there's no demo token" do
    Application.put_env(:presence_relay, :demo, false)

    try do
      assert json_response(
               post(build_conn(), "/api/demo/token", %{"room" => "r", "name" => "J"}),
               404
             )
    after
      Application.put_env(:presence_relay, :demo, true)
    end
  end

  test "healthz, the demo's index for its routes, and no index for /api" do
    assert response(get(build_conn(), "/healthz"), 200) == "ok"
    assert response(get(build_conn(), "/some/room"), 200) =~ "arugula.video"
    assert response(get(build_conn(), "/api/nope"), 404)
    _ = put_req_header(build_conn(), "accept", "application/json")
  end

  test "the SDK, its models and demo tokens answer any origin; nothing else does" do
    for path <- ~w(/sdk/v0/nope.js /mediapipe/nope.task /depth/nope.onnx) do
      conn = get(build_conn(), path)
      assert get_resp_header(conn, "access-control-allow-origin") == ["*"], path
      # A missing module is a 404, not the demo's index in its place.
      assert conn.status == 404, path
    end

    conn = post(build_conn(), "/api/demo/token", %{"room" => "standup", "name" => "Jake"})
    assert get_resp_header(conn, "access-control-allow-origin") == ["*"]

    assert get_resp_header(get(build_conn(), "/healthz"), "access-control-allow-origin") == []
    assert get_resp_header(get(build_conn(), "/some/room"), "access-control-allow-origin") == []
  end

  test "the demo token's preflight allows a JSON POST from anywhere" do
    conn =
      build_conn()
      |> put_req_header("origin", "https://example.com")
      |> put_req_header("access-control-request-method", "POST")
      |> options("/api/demo/token")

    assert conn.status == 204
    assert get_resp_header(conn, "access-control-allow-origin") == ["*"]
    assert get_resp_header(conn, "access-control-allow-methods") == ["POST"]
    assert get_resp_header(conn, "access-control-allow-headers") == ["content-type"]
  end

  test "go get's lookup of the vanity path names the module proxy, and the hosted dirs 404 when empty" do
    conn = build_conn() |> Map.put(:host, "arugula.stream") |> get("/presence?go-get=1")
    assert conn.status == 200

    assert conn.resp_body =~
             ~s(<meta name="go-import" content="arugula.stream/presence mod https://arugula.stream/go">)

    assert build_conn() |> get("/go/arugula.stream/presence/@v/v9.9.9.zip") |> Map.get(:status) ==
             404

    assert build_conn() |> get("/npm/nothing.tgz") |> Map.get(:status) == 404
  end
end
