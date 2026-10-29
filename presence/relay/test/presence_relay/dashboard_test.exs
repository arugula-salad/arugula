defmodule PresenceRelay.DashboardTest do
  @moduledoc "The dashboard (#318 P10): where it's served, and the API calls it makes as it makes them."
  use PresenceRelay.DataCase, async: false
  import Plug.Conn
  import Phoenix.ConnTest

  @endpoint PresenceRelayWeb.Endpoint

  alias PresenceRelay.Accounts.Account

  defp as(%Account{id: id}), do: build_conn() |> init_test_session(account_id: id)

  defp json(conn, method, path, body) do
    conn
    |> put_req_header("content-type", "application/json")
    |> dispatch(@endpoint, method, path, Jason.encode!(body))
  end

  describe "routing" do
    test "under /dashboard, the dashboard's page for every route" do
      for path <- ~w(/dashboard /dashboard/ /dashboard/apps/app_0123456789abcdef) do
        assert response(get(build_conn(), path), 200) =~ "dashboard test index", path
      end
    end

    test "a missing build file is missing; everything else is as before" do
      assert response(get(build_conn(), "/dashboard/assets/nope.js"), 404)

      for path <- ~w(/ /dashboards /docs /hosted /r/dashboard) do
        assert response(get(build_conn(), path), 200) =~ "test index", path
        refute response(get(build_conn(), path), 200) =~ "dashboard", path
      end

      # A meet. host is meet's, all of it.
      meet = %{build_conn() | host: "meet.arugula.stream"} |> get("/dashboard")
      assert response(meet, 200) =~ "meet test index"
    end
  end

  test "an app set up the way the dashboard does it" do
    owner =
      Repo.insert!(%Account{
        id: "acc_d#{System.unique_integer([:positive])}",
        email: "maya@example.com"
      })

    %{"app" => %{"id" => id}, "secret" => _} =
      as(owner) |> json(:post, "/api/v1/apps", %{name: "Sketchpad"}) |> json_response(201)

    %{"key" => %{"key" => "pk_test_" <> _ = key}} =
      as(owner) |> json(:post, "/api/v1/apps/#{id}/keys", %{mode: "test"}) |> json_response(201)

    as(owner)
    |> json(:post, "/api/v1/apps/#{id}/origins", %{origin: "https://*.sketchpad.app"})
    |> json_response(200)

    # Origins come off by a JSON body, as the dashboard sends them.
    assert %{"app" => %{"origins" => []}} =
             as(owner)
             |> json(:delete, "/api/v1/apps/#{id}/origins", %{origin: "https://*.sketchpad.app"})
             |> json_response(200)

    assert %{"app" => %{"guests" => true}} =
             as(owner)
             |> json(:patch, "/api/v1/apps/#{id}", %{guests: true})
             |> json_response(200)

    # The room rule rides on the issuer; saving it sends the issuer back.
    issuer = %{provider: "oidc", issuer: "https://id.sketchpad.app", audience: "sketchpad"}

    assert %{"error" => "a room rule's match is prefix or contains"} =
             as(owner)
             |> json(
               :put,
               "/api/v1/apps/#{id}/issuer",
               Map.put(issuer, :room_rule, %{claim: "org", match: "starts"})
             )
             |> json_response(400)

    assert %{
             "issuer" => %{
               "room_rule" => %{"claim" => "org", "match" => "prefix"},
               "iss" => "https://id.sketchpad.app"
             }
           } =
             as(owner)
             |> json(
               :put,
               "/api/v1/apps/#{id}/issuer",
               Map.put(issuer, :room_rule, %{claim: "org", match: "prefix"})
             )
             |> json_response(200)

    assert %{"app" => %{"keys" => keys}} =
             as(owner) |> get("/api/v1/apps/#{id}") |> json_response(200)

    assert key in Enum.map(keys, & &1["key"])
  end
end
