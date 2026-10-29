defmodule PresenceRelay.AccountsHttpTest do
  use PresenceRelay.DataCase, async: false
  import Plug.Conn
  import Phoenix.ConnTest
  import Swoosh.TestAssertions

  @endpoint PresenceRelayWeb.Endpoint

  defp json_post(conn, path, body),
    do:
      conn
      |> put_req_header("content-type", "application/json")
      |> post(path, Jason.encode!(body))

  # A link emailed to `email`: the conn that asked (its session) and the token.
  defp request_link(email) do
    conn = json_post(build_conn(), "/api/v1/auth/email", %{email: email, return_to: "/hosted"})
    assert json_response(conn, 200) == %{"sent" => true}

    assert_email_sent(fn e ->
      [_, token] =
        Regex.run(~r{http://www\.example\.com/auth/email/([A-Za-z0-9_-]+)}, e.text_body)

      send(self(), {:token, token})
      true
    end)

    assert_received {:token, token}
    {recycle(conn), token}
  end

  # The link's page: the conn after it and the form's CSRF token.
  defp open_link(conn, token) do
    conn = get(conn, "/auth/email/#{token}")
    body = html_response(conn, 200)
    assert body =~ ~s(action="/auth/email/#{token}")
    [_, csrf] = Regex.run(~r{name="_csrf_token" value="([^"]+)"}, body)
    {recycle(conn), csrf}
  end

  # The page's button, with CSRF checked (ConnTest skips it by default).
  defp press(conn, token, csrf) do
    params = if csrf, do: %{"_csrf_token" => csrf}, else: %{}

    conn
    |> put_private(:plug_skip_csrf_protection, false)
    |> post("/auth/email/#{token}", params)
  end

  defp sign_in(email \\ "ada@example.com") do
    {conn, token} = request_link(email)
    {conn, csrf} = open_link(conn, token)
    conn = press(conn, token, csrf)
    assert redirected_to(conn) == "/hosted"
    recycle(conn)
  end

  test "opening the link spends nothing (mail scanners open links); its button signs in, once" do
    {conn, token} = request_link("ada@example.com")
    {conn, _} = open_link(conn, token)
    assert get(conn, "/auth/email/#{token}") |> html_response(200) =~ "ada@example.com"
    {conn, csrf} = open_link(conn, token)

    signed = press(conn, token, csrf)
    assert redirected_to(signed) == "/hosted"

    assert %{"account" => %{"email" => "ada@example.com"}} =
             signed |> recycle() |> get("/api/v1/me") |> json_response(200)

    assert conn |> recycle() |> get("/auth/email/#{token}") |> response(400) =~ "already used"

    assert conn
           |> recycle()
           |> press(token, csrf)
           |> response(400)
  end

  test "the button without its CSRF token is refused, and the link still works" do
    {conn, token} = request_link("ada@example.com")
    {conn, csrf} = open_link(conn, token)

    assert_error_sent 403, fn -> press(build_conn(), token, nil) end
    assert_error_sent 403, fn -> press(conn, token, nil) end
    assert_error_sent 403, fn -> press(build_conn(), token, csrf) end

    assert conn
           |> recycle()
           |> press(token, csrf)
           |> redirected_to() == "/hosted"
  end

  test "an expired or made-up link is a 400 either way" do
    {conn, token} = request_link("ada@example.com")
    {conn, csrf} = open_link(conn, token)

    PresenceRelay.Repo.update_all(PresenceRelay.Accounts.LoginToken,
      set: [expires_at: DateTime.add(DateTime.utc_now(), -60) |> DateTime.truncate(:second)]
    )

    for t <- [token, "made-up"] do
      assert conn |> recycle() |> get("/auth/email/#{t}") |> response(400) =~ "expired"

      assert conn
             |> recycle()
             |> press(t, csrf)
             |> response(400)
    end
  end

  test "the page escapes the address" do
    {conn, token} = request_link("o'neil+x@example.com")
    body = conn |> get("/auth/email/#{token}") |> html_response(200)
    assert body =~ "o&#39;neil+x@example.com"
  end

  test "an emailed link signs in; /me says who; logout signs out" do
    conn = sign_in()

    assert %{"account" => %{"email" => "ada@example.com"}} =
             conn |> get("/api/v1/me") |> json_response(200)

    conn = conn |> recycle() |> json_post("/api/v1/auth/logout", %{})
    assert json_response(conn, 200)["signed_out"]
    assert conn |> recycle() |> get("/api/v1/me") |> json_response(401)
  end

  test "the apps API: make, list, origins, keys, rotate; never the secret twice" do
    conn = sign_in()
    made = conn |> json_post("/api/v1/apps", %{name: "Acme", guests: true}) |> json_response(201)
    %{"app" => %{"id" => id, "keys" => [%{"key" => "pk_live_" <> _}]}, "secret" => secret} = made
    assert is_binary(secret)

    conn = recycle(conn)
    assert %{"apps" => [%{"id" => ^id} = app]} = conn |> get("/api/v1/apps") |> json_response(200)
    refute Map.has_key?(app, "secret")

    conn = recycle(conn)

    assert %{"app" => %{"origins" => ["https://acme.com"]}} =
             conn
             |> json_post("/api/v1/apps/#{id}/origins", %{origin: "https://acme.com"})
             |> json_response(200)

    conn = recycle(conn)

    assert %{"error" => _} =
             conn
             |> json_post("/api/v1/apps/#{id}/origins", %{origin: "nope"})
             |> json_response(400)

    conn = recycle(conn)

    %{"key" => %{"key" => test_key, "mode" => "test"}} =
      conn |> json_post("/api/v1/apps/#{id}/keys", %{mode: "test"}) |> json_response(201)

    conn = recycle(conn)
    assert conn |> delete("/api/v1/apps/#{id}/keys/#{test_key}") |> json_response(200)

    conn = recycle(conn)

    %{"secret" => new_secret} =
      conn |> json_post("/api/v1/apps/#{id}/secret", %{}) |> json_response(200)

    refute new_secret == secret

    conn = recycle(conn)

    assert %{"app" => %{"origins" => []}} =
             conn
             |> delete("/api/v1/apps/#{id}/origins?origin=https://acme.com")
             |> json_response(200)
  end

  test "someone else's app is a 404; signed out is a 401; a form post is refused" do
    conn = sign_in("ada@example.com")

    %{"app" => %{"id" => id}} =
      conn |> json_post("/api/v1/apps", %{name: "Acme"}) |> json_response(201)

    bob = sign_in("bob@example.com")
    assert bob |> get("/api/v1/apps/#{id}") |> json_response(404)
    assert build_conn() |> get("/api/v1/apps") |> json_response(401)

    form =
      bob
      |> recycle()
      |> put_req_header("content-type", "application/x-www-form-urlencoded")
      |> post("/api/v1/apps", "name=x")

    assert form.status == 415
  end

  test "Google: off to Google with a state, back with a code, signed in; a wrong state isn't" do
    conn = get(build_conn(), "/auth/google?return_to=/hosted")
    url = redirected_to(conn, 302)
    assert url =~ "accounts.google.com"
    %{"state" => state, "redirect_uri" => redirect} = URI.decode_query(URI.parse(url).query)
    assert redirect == "http://www.example.com/auth/google/callback"

    bad = conn |> recycle() |> get("/auth/google/callback?code=good&state=wrong")
    assert bad.status == 400

    conn = conn |> recycle() |> get("/auth/google/callback?code=good&state=#{state}")
    assert redirected_to(conn) == "/hosted"

    assert %{"account" => %{"email" => "ada@example.com"}} =
             conn |> recycle() |> get("/api/v1/me") |> json_response(200)
  end

  test "the providers say what works here" do
    assert %{"email" => true, "google" => true, "guests" => true} =
             build_conn() |> get("/api/v1/auth/providers") |> json_response(200)
  end
end
