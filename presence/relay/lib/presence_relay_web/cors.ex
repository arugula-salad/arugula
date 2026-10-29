defmodule PresenceRelayWeb.Cors do
  @moduledoc """
  What a page on another origin needs from the relay to embed presence
  (the SDK's script tag): the SDK's modules (`/sdk/**`), the models and Wasm
  its trackers fetch (`/mediapipe/**`, `/depth/**`), and demo tokens
  (`POST /api/demo/token`, with its preflight). Those, and nothing else,
  answer any origin: none of it carries a cookie or anything private.
  """

  import Plug.Conn

  @behaviour Plug

  @open ~w(sdk mediapipe depth)

  @impl true
  def init(opts), do: opts

  @impl true
  def call(%Plug.Conn{path_info: [top | _]} = conn, _) when top in @open, do: allow(conn)

  def call(%Plug.Conn{method: "OPTIONS", path_info: ["api", "demo", "token"]} = conn, _) do
    conn
    |> allow()
    |> put_resp_header("access-control-allow-methods", "POST")
    |> put_resp_header("access-control-allow-headers", "content-type")
    |> put_resp_header("access-control-max-age", "86400")
    |> send_resp(204, "")
    |> halt()
  end

  def call(%Plug.Conn{path_info: ["api", "demo", "token"]} = conn, _), do: allow(conn)

  def call(conn, _), do: conn

  defp allow(conn), do: put_resp_header(conn, "access-control-allow-origin", "*")
end
