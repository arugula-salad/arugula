defmodule PresenceRelayWeb.MetricsPlug do
  @moduledoc """
  `GET /metrics` (`PresenceRelay.Metrics`), alone on its own port
  (config `:metrics_port`, `METRICS_PORT`: 9091 in prod), which no Fly
  service exposes: only Fly's scraper, on the private network, reaches
  it. Everything else there is a 404.
  """
  import Plug.Conn

  @behaviour Plug

  @impl true
  def init(opts), do: opts

  @impl true
  def call(%Plug.Conn{method: "GET", path_info: ["metrics"]} = conn, _) do
    conn
    |> put_resp_content_type("text/plain; version=0.0.4", nil)
    |> send_resp(200, PresenceRelay.Metrics.render())
  end

  def call(conn, _), do: send_resp(conn, 404, "")
end
