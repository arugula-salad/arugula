defmodule PresenceRelayWeb.Auth do
  @moduledoc "The signed-in account, from the session cookie; and the guard on JSON writes."
  import Plug.Conn
  import Phoenix.Controller, only: [json: 2]

  def fetch_account(conn, _) do
    account =
      if PresenceRelay.Apps.database?(),
        do: PresenceRelay.Accounts.get(get_session(conn, :account_id))

    assign(conn, :account, account)
  end

  def require_account(%{assigns: %{account: %{}}} = conn, _), do: conn

  def require_account(conn, _),
    do: conn |> put_status(401) |> json(%{error: "sign in first"}) |> halt()

  @doc "POST and PATCH must be JSON: a cross-site form can only send form encodings."
  def json_writes(%{method: m} = conn, _) when m in ["POST", "PATCH", "PUT"] do
    case get_req_header(conn, "content-type") do
      ["application/json" <> _] -> conn
      _ -> conn |> put_status(415) |> json(%{error: "send JSON"}) |> halt()
    end
  end

  def json_writes(conn, _), do: conn

  @doc """
  This request's own origin, as the browser sees it: behind Fly's proxy
  (which terminates TLS) the forwarded scheme on the standard port.
  """
  def base_url(conn) do
    case get_req_header(conn, "x-forwarded-proto") do
      [proto | _] ->
        "#{proto}://#{conn.host}"

      [] ->
        default = if conn.scheme == :https, do: 443, else: 80
        port = if conn.port == default, do: "", else: ":#{conn.port}"
        "#{conn.scheme}://#{conn.host}#{port}"
    end
  end

  @doc "A path to go back to after signing in: same site only."
  def return_to(path) when is_binary(path) do
    if String.starts_with?(path, "/") and not String.starts_with?(path, "//"), do: path, else: "/"
  end

  def return_to(_), do: "/"
end
