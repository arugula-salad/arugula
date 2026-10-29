defmodule PresenceRelayWeb.AuthController do
  @moduledoc """
  Signing in (#318 P3): an emailed link, or Google. Guests don't sign in
  (a guest is a name on a socket). The session cookie carries the account.
  """
  use Phoenix.Controller, formats: [:json]

  alias PresenceRelay.Accounts
  alias PresenceRelay.Accounts.Google
  import PresenceRelayWeb.Auth, only: [base_url: 1, return_to: 1]

  @doc "`POST /api/v1/auth/email {email, return_to?}`: emails a sign-in link."
  def email_request(conn, params) do
    with :ok <- no_db(conn) do
      case Accounts.send_link(params["email"], base_url(conn)) do
        :ok ->
          conn |> put_session(:return_to, return_to(params["return_to"])) |> json(%{sent: true})

        {:error, :bad_email} ->
          conn |> put_status(400) |> json(%{error: "that doesn't look like an email address"})

        {:error, :no_mailer} ->
          conn |> put_status(503) |> json(%{error: "email sign-in isn't set up here"})

        {:error, _} ->
          conn |> put_status(502) |> json(%{error: "the email didn't send; try again"})
      end
    end
  end

  @doc """
  `GET /auth/email/:token`: the link. It spends nothing (mail scanners
  fetch links before people click them): a page with one button, which
  posts back here.
  """
  def email_confirm(conn, %{"token" => token}) do
    case Accounts.check_link(token) do
      {:ok, email} ->
        conn
        |> put_resp_header("cache-control", "no-store")
        |> put_resp_header("referrer-policy", "no-referrer")
        |> html(confirm_page(email, token, get_csrf_token()))

      {:error, _} ->
        bad_link(conn)
    end
  end

  @doc "`POST /auth/email/:token`: the page's button; signs in and goes back."
  def email_callback(conn, %{"token" => token}) do
    case Accounts.use_link(token) do
      {:ok, account} -> signed_in(conn, account)
      {:error, _} -> bad_link(conn)
    end
  end

  defp bad_link(conn),
    do: conn |> put_status(400) |> text("That sign-in link has expired or was already used.")

  defp confirm_page(email, token, csrf) do
    e = &(&1 |> Plug.HTML.html_escape() |> IO.iodata_to_binary())

    """
    <!doctype html>
    <html lang="en">
    <head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <meta name="robots" content="noindex">
    <title>Sign in · arugula.stream</title>
    <style>
    body{margin:0;min-height:100vh;display:grid;place-items:center;background:#0a0d14;color:#e6e9ef;font:16px/1.5 system-ui,sans-serif;padding:16px;box-sizing:border-box}
    main{max-width:420px;width:100%;border:1px solid #1d2433;border-radius:12px;padding:28px;text-align:center}
    h1{font-size:20px;font-weight:600;margin:0 0 20px}
    b{color:#4fd1c5;font-weight:600;overflow-wrap:anywhere}
    button{font:inherit;font-weight:600;background:#4fd1c5;color:#0a0d14;border:0;border-radius:8px;padding:10px 22px;cursor:pointer}
    button:focus-visible{outline:2px solid #e6e9ef;outline-offset:3px}
    </style>
    </head>
    <body>
    <main>
    <h1>Sign in to arugula.stream as <b>#{e.(email)}</b></h1>
    <form method="post" action="/auth/email/#{e.(token)}">
    <input type="hidden" name="_csrf_token" value="#{e.(csrf)}">
    <button type="submit">Sign in</button>
    </form>
    </main>
    </body>
    </html>
    """
  end

  @doc "`GET /auth/google?return_to=`: off to Google."
  def google(conn, params) do
    if Google.enabled?() do
      state = Base.url_encode64(:crypto.strong_rand_bytes(18), padding: false)

      conn
      |> put_session(:google_state, state)
      |> put_session(:return_to, return_to(params["return_to"]))
      |> redirect(
        external: Google.authorize_url(state, base_url(conn) <> "/auth/google/callback")
      )
    else
      conn |> put_status(404) |> text("Google sign-in isn't set up here.")
    end
  end

  @doc "`GET /auth/google/callback`: Google's answer."
  def google_callback(conn, %{"code" => code, "state" => state}) do
    with true <- Google.enabled?() || :off,
         expected when is_binary(expected) <- get_session(conn, :google_state),
         true <- Plug.Crypto.secure_compare(expected, state) || :state,
         {:ok, profile} <- Google.profile(code, base_url(conn) <> "/auth/google/callback"),
         {:ok, account} <- Accounts.from_google(profile) do
      conn |> delete_session(:google_state) |> signed_in(account)
    else
      _ -> conn |> put_status(400) |> text("Google sign-in didn't work. Try again.")
    end
  end

  def google_callback(conn, _),
    do: conn |> put_status(400) |> text("Google sign-in was cancelled.")

  @doc "`POST /api/v1/auth/logout`."
  def logout(conn, _), do: conn |> configure_session(drop: true) |> json(%{signed_out: true})

  @doc "`GET /api/v1/auth/providers`: which ways in work here."
  def providers(conn, _) do
    json(conn, %{email: PresenceRelay.Mailer.ready?(), google: Google.enabled?(), guests: true})
  end

  defp signed_in(conn, account) do
    back = get_session(conn, :return_to) || "/"

    conn
    |> configure_session(renew: true)
    |> put_session(:account_id, account.id)
    |> delete_session(:return_to)
    |> redirect(to: return_to(back))
  end

  defp no_db(conn) do
    if PresenceRelay.Apps.database?(),
      do: :ok,
      else: conn |> put_status(503) |> json(%{error: "no accounts here"}) |> halt()
  end
end
