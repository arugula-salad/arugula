defmodule PresenceRelayWeb.PageController do
  use Phoenix.Controller, formats: [:html, :json]

  # `?db=1` also asks Postgres (`select 1`): "ok db", or 503 when it doesn't
  # answer; "ok no-db" for a relay without one. The machine check uses the
  # plain form, so a database blip doesn't take the relay out.
  def healthz(conn, %{"db" => _}) do
    cond do
      not Application.get_env(:presence_relay, :database, true) ->
        text(conn, "ok no-db")

      match?({:ok, _}, Ecto.Adapters.SQL.query(PresenceRelay.Repo, "select 1", [])) ->
        text(conn, "ok db")

      true ->
        conn |> put_status(503) |> text("db down")
    end
  end

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
    # The demo's build (git ignores it); tests point at a fixture instead.
    path =
      Application.get_env(:presence_relay, :index_html) ||
        Application.app_dir(:presence_relay, "priv/static/index.html")

    if File.exists?(path) do
      conn |> put_resp_content_type("text/html") |> send_file(200, path)
    else
      send_resp(conn, 404, "")
    end
  end
end
