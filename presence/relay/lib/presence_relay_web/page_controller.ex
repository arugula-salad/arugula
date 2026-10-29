defmodule PresenceRelayWeb.PageController do
  use Phoenix.Controller, formats: [:html, :json]

  # `?deep=1` (#318's P12): every deep check (PresenceRelay.Health) as
  # JSON, 503 when any fails; for people and the watchdog, not Fly's
  # machine check.
  def healthz(conn, %{"deep" => _}) do
    h = PresenceRelay.Health.deep()
    conn |> put_status(if(h.ok, do: 200, else: 503)) |> json(h)
  end

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

  # Likewise meet's and the dashboard's built files: a missing one isn't
  # their HTML.
  def index(conn, %{"path" => [app, "assets" | _]}) when app in ~w(meet dashboard),
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
    # The demo's build, meet's or the dashboard's (git ignores them all);
    # tests point at fixtures instead.
    path =
      cond do
        meet?(conn) ->
          Application.get_env(:presence_relay, :meet_index_html) ||
            Application.app_dir(:presence_relay, "priv/static/meet/index.html")

        dashboard?(conn) ->
          Application.get_env(:presence_relay, :dashboard_index_html) ||
            Application.app_dir(:presence_relay, "priv/static/dashboard/index.html")

        true ->
          Application.get_env(:presence_relay, :index_html) ||
            Application.app_dir(:presence_relay, "priv/static/index.html")
      end

    if File.exists?(path) do
      conn |> put_resp_content_type("text/html") |> send_file(200, path)
    else
      send_resp(conn, 404, "")
    end
  end

  @doc """
  Whether a page is meet.arugula.stream's (#318 P6): on a host starting
  `meet.`, or under `/meet` on any host (staging, before meet's DNS). Its
  build (priv/static/meet) works under either: its files are always under
  /meet, and it reads its base from the page's path.
  """
  def meet?(%Plug.Conn{host: "meet." <> _}), do: true
  def meet?(%Plug.Conn{path_info: ["meet" | _]}), do: true
  def meet?(_), do: false

  @doc """
  Whether a page is the dashboard's (#318 P10): under `/dashboard`, on any
  host but meet's (staging too). Its build is priv/static/dashboard.
  """
  def dashboard?(%Plug.Conn{path_info: ["dashboard" | _]} = conn), do: not meet?(conn)
  def dashboard?(_), do: false
end
