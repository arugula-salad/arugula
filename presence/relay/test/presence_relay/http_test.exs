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
end
