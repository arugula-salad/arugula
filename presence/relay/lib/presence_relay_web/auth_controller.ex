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

  @doc "`GET /auth/email/:token`: the link; signs in and goes back."
  def email_callback(conn, %{"token" => token}) do
    case Accounts.use_link(token) do
      {:ok, account} ->
        signed_in(conn, account)

      {:error, _} ->
        conn |> put_status(400) |> text("That sign-in link has expired or was already used.")
    end
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
