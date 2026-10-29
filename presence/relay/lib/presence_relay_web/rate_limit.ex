defmodule PresenceRelayWeb.RateLimit do
  @moduledoc """
  An abuse limit on HTTP requests (`PresenceRelay.Limits`), by client IP:
  `plug PresenceRelayWeb.RateLimit, :demo_token`. Over it, a 429 with
  `Retry-After` (seconds) and `{error}` saying when to try again.

  `{name, writes: true}` counts only POST, PUT, PATCH and DELETE.
  """
  import Plug.Conn

  alias PresenceRelay.Limits
  alias PresenceRelayWeb.ClientIP

  @behaviour Plug
  @writes ~w(POST PUT PATCH DELETE)

  @impl true
  def init({name, opts}), do: {name, Keyword.get(opts, :writes, false)}
  def init(name) when is_atom(name), do: {name, false}

  @impl true
  def call(%Plug.Conn{method: m} = conn, {_, true}) when m not in @writes, do: conn

  def call(conn, {name, _}) do
    case Limits.hit(name, ClientIP.of(conn)) do
      :ok -> conn
      {:error, retry} -> refuse(conn, retry)
    end
  end

  @doc "A 429 with `Retry-After`, halted."
  def refuse(conn, retry) do
    conn
    |> put_resp_header("retry-after", Integer.to_string(retry))
    |> put_resp_content_type("application/json")
    |> send_resp(429, Jason.encode!(%{error: too_many(retry)}))
    |> halt()
  end

  @doc "What a client over a limit is told."
  def too_many(retry), do: "too many requests; try again in #{retry} s"
end
