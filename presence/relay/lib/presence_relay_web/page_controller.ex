defmodule PresenceRelayWeb.PageController do
  use Phoenix.Controller, formats: [:html, :json]

  def healthz(conn, _), do: text(conn, "ok")

  # Files are Plug.Static's; a path that isn't one is the demo's route.
  def index(conn, %{"path" => ["api" | _]}), do: send_resp(conn, 404, "")

  def index(conn, _) do
    path = Application.app_dir(:presence_relay, "priv/static/index.html")

    if File.exists?(path) do
      conn |> put_resp_content_type("text/html") |> send_file(200, path)
    else
      send_resp(conn, 404, "")
    end
  end
end
