defmodule PresenceRelay.Accounts.Google do
  @moduledoc """
  Sign in with Google (OAuth 2 / OpenID Connect, the code flow). On when
  `GOOGLE_CLIENT_ID` and `GOOGLE_CLIENT_SECRET` are set (runtime.exs).
  The profile comes from Google's userinfo endpoint over TLS with the
  access token the code was exchanged for, so there's no ID-token
  signature to check here. The HTTP is `client/0`'s, a fake in tests.
  """

  @authorize "https://accounts.google.com/o/oauth2/v2/auth"

  @callback exchange(code :: String.t(), redirect_uri :: String.t(), config :: keyword) ::
              {:ok, map} | {:error, term}

  def config, do: Application.get_env(:presence_relay, __MODULE__, [])
  def enabled?, do: is_binary(config()[:client_id]) and is_binary(config()[:client_secret])

  @doc "Where to send the browser, carrying `state` (kept in its session)."
  def authorize_url(state, redirect_uri) do
    @authorize <>
      "?" <>
      URI.encode_query(%{
        "client_id" => config()[:client_id],
        "redirect_uri" => redirect_uri,
        "response_type" => "code",
        "scope" => "openid email profile",
        "state" => state,
        "prompt" => "select_account"
      })
  end

  @doc "The profile for an authorization code: `{:ok, %{\"sub\", \"email\", …}}`."
  def profile(code, redirect_uri), do: client().exchange(code, redirect_uri, config())

  defp client, do: config()[:client] || __MODULE__.HTTP
end

defmodule PresenceRelay.Accounts.Google.HTTP do
  @moduledoc "Google's token and userinfo endpoints, with OTP's :httpc."
  @behaviour PresenceRelay.Accounts.Google

  @token "https://oauth2.googleapis.com/token"
  @userinfo "https://openidconnect.googleapis.com/v1/userinfo"

  @impl true
  def exchange(code, redirect_uri, config) do
    body =
      URI.encode_query(%{
        "code" => code,
        "client_id" => config[:client_id],
        "client_secret" => config[:client_secret],
        "redirect_uri" => redirect_uri,
        "grant_type" => "authorization_code"
      })

    with {:ok, %{"access_token" => access}} <-
           request(:post, @token, [], {~c"application/x-www-form-urlencoded", body}),
         {:ok, profile} <-
           request(
             :get,
             @userinfo,
             [{~c"authorization", ~c"Bearer " ++ to_charlist(access)}],
             nil
           ) do
      {:ok, profile}
    else
      {:ok, other} -> {:error, {:unexpected, other}}
      error -> error
    end
  end

  defp request(method, url, headers, body) do
    :ok = Application.ensure_started(:inets)
    {:ok, _} = Application.ensure_all_started(:ssl)

    ssl = [
      verify: :verify_peer,
      cacerts: :public_key.cacerts_get(),
      customize_hostname_check: [match_fun: :public_key.pkix_verify_hostname_match_fun(:https)]
    ]

    req =
      if body,
        do: {to_charlist(url), headers, elem(body, 0), elem(body, 1)},
        else: {to_charlist(url), headers}

    case :httpc.request(method, req, [ssl: ssl, timeout: 10_000], body_format: :binary) do
      {:ok, {{_, 200, _}, _, resp}} -> Jason.decode(resp)
      {:ok, {{_, status, _}, _, resp}} -> {:error, {status, resp}}
      {:error, reason} -> {:error, reason}
    end
  end
end
