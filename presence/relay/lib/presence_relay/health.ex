defmodule PresenceRelay.Health do
  @moduledoc """
  The deep health check (#318's P12): `GET /healthz?deep=1`, and what the
  watchdog (`PresenceRelay.Watchdog`) asks every node.

  - `db`: Postgres answers `select 1` within 2 s (a relay without a
    database skips it).
  - `usage`: a usage flush has left nothing behind within
    `usage_stuck_s` (120 s; flushes run every 5 s), so usage isn't piling
    up unwritten (skipped where flushes are manual, in tests).

  The machine check stays the plain `/healthz`: a database blip mustn't
  take the relay out of Fly's routing, since rooms work without it.
  """

  alias PresenceRelay.{Metrics, Repo}

  @doc "`%{ok, node, nodes, checks}`: `ok` when every check is."
  def deep do
    checks = %{db: db(), usage: usage()}

    %{
      ok: Enum.all?(checks, fn {_, c} -> c.ok end),
      node: to_string(node()),
      nodes: length(Node.list()) + 1,
      checks: checks
    }
  end

  defp db do
    if Application.get_env(:presence_relay, :database, true) do
      t0 = System.monotonic_time(:millisecond)

      case Ecto.Adapters.SQL.query(Repo, "select 1", [], timeout: 2_000) do
        {:ok, _} -> %{ok: true, ms: System.monotonic_time(:millisecond) - t0}
        {:error, e} -> %{ok: false, error: Exception.message(e)}
      end
    else
      %{ok: true, skipped: "no database"}
    end
  rescue
    e -> %{ok: false, error: Exception.message(e)}
  catch
    :exit, _ -> %{ok: false, error: "no answer"}
  end

  defp usage do
    if is_integer(Application.get_env(:presence_relay, :usage_flush_ms, 5_000)) do
      s = Metrics.usage_unflushed_seconds()
      stuck = Application.get_env(:presence_relay, :usage_stuck_s, 120)
      %{ok: s < stuck, unflushed_s: Float.round(s * 1.0, 1)}
    else
      %{ok: true, skipped: "manual flushes"}
    end
  end
end
