defmodule PresenceRelayWeb.PageController do
  use Phoenix.Controller, formats: [:html, :json]

  def healthz(conn, _), do: text(conn, "ok")

  # Files are Plug.Static's; a path that isn't one is the demo's route,
  # except under the API and the SDK's and models' directories, where a
  # missing file is missing (not the demo's HTML in a module's place).
  def index(conn, %{"path" => [top | _]}) when top in ~w(api sdk mediapipe depth npm py go gems),
    do: send_resp(conn, 404, "")

  # `go get arugula.stream/presence`: the module is served by the module
  # proxy protocol from /go (presence/sdk/host.sh), so the import path
  # names it with the "mod" kind.
  def index(conn, %{"path" => ["presence" | _], "go-get" => "1"}) do
    host = conn.host

    conn
    |> put_resp_content_type("text/html")
    |> send_resp(200, """
    <!doctype html><meta name="go-import" content="#{host}/presence mod https://#{host}/go">
    """)
  end

  def index(conn, _) do
    # The demo's build (priv/static, git-ignored); tests point at a fixture.
    path =
      Application.get_env(:presence_relay, :index_path) ||
        Application.app_dir(:presence_relay, "priv/static/index.html")

    if File.exists?(path) do
      conn |> put_resp_content_type("text/html") |> send_file(200, path)
    else
      send_resp(conn, 404, "")
    end
  end
end
