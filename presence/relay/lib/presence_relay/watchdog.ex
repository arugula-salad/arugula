defmodule PresenceRelay.Watchdog do
  @moduledoc """
  Mails someone when the relay is unwell (#318's P12), with the mailer
  it already has (Resend), so alerting needs no other service.

  Every `watchdog_ms` (a minute) one node, the cluster's first by name,
  asks every node for its deep health (`PresenceRelay.Health`). After
  `alert_after` (3) failing checks in a row it mails `alert_to`
  (`ALERT_EMAIL`, by default hello@arugula.stream) what's failing where;
  again every `alert_repeat_ms` (6 h) while it lasts; and once more when
  every node is well again. A node that doesn't answer counts as failing.

  It can't tell anyone the whole app is down (nothing's left to send the
  mail): Fly's machine checks restart a dead machine, and presence/ops/
  README.md has the Grafana alert for "no machine is up".

  No mailer (no `RESEND_API_KEY`), and the alert goes to the log only.
  Off where `watchdog_ms` isn't a number (tests).
  """
  use GenServer
  require Logger
  import Swoosh.Email

  alias PresenceRelay.{Health, Mailer}

  def start_link(_), do: GenServer.start_link(__MODULE__, nil, name: __MODULE__)

  @doc false
  # Tests: one check now, as if the interval had passed.
  def check, do: GenServer.call(__MODULE__, :check)

  @impl true
  def init(_) do
    schedule()
    {:ok, %{failing: 0, alerted_at: nil}}
  end

  @impl true
  def handle_info(:check, s) do
    schedule()
    {:noreply, run(s)}
  end

  @impl true
  def handle_call(:check, _from, s) do
    s = run(s)
    {:reply, s, s}
  end

  defp run(s) do
    if leader?(), do: step(s, sick()), else: %{s | failing: 0}
  end

  # The cluster's first node by name checks; the others don't, so one
  # mail goes out (a split cluster may send two).
  defp leader?, do: node() == Enum.min([node() | Node.list()])

  # Each node's failing checks: [{node, why}], empty when all is well.
  defp sick do
    nodes = [node() | Node.list()]

    nodes
    |> :erpc.multicall(Health, :deep, [], 5_000)
    |> Enum.zip(nodes)
    |> Enum.flat_map(fn
      {{:ok, %{ok: true}}, _} ->
        []

      {{:ok, %{checks: checks}}, n} ->
        for {name, %{ok: false} = c} <- checks, do: {n, "#{name}: #{why(c)}"}

      {_, n} ->
        [{n, "didn't answer"}]
    end)
  end

  # A failing check in words: its error, else what it measured.
  defp why(%{error: e}), do: e
  defp why(c), do: c |> Map.delete(:ok) |> Enum.map_join(", ", fn {k, v} -> "#{k} #{v}" end)

  defp step(s, []) do
    if s.alerted_at, do: mail("arugula.stream relay: well again", "Every node is well again.")
    %{s | failing: 0, alerted_at: nil}
  end

  defp step(s, sick) do
    failing = s.failing + 1
    now = System.monotonic_time(:millisecond)
    repeat = Application.get_env(:presence_relay, :alert_repeat_ms, 6 * 3600 * 1000)

    due =
      failing >= Application.get_env(:presence_relay, :alert_after, 3) and
        (is_nil(s.alerted_at) or now - s.alerted_at >= repeat)

    if due do
      lines = Enum.map_join(sick, "\n", fn {n, why} -> "- #{n}: #{why}" end)

      mail(
        "arugula.stream relay: health failing",
        "The relay's deep health check has failed #{failing} times in a row:\n\n#{lines}\n\n" <>
          "Look: fly logs, and Grafana's arugula.stream dashboard."
      )

      %{s | failing: failing, alerted_at: now}
    else
      %{s | failing: failing}
    end
  end

  defp mail(subject, body) do
    Logger.warning("watchdog: #{subject}: #{body}")

    if Mailer.ready?() do
      new()
      |> to(Application.get_env(:presence_relay, :alert_to, "hello@arugula.stream"))
      |> from(Application.get_env(:presence_relay, :mail_from))
      |> subject(subject)
      |> text_body(body)
      |> Mailer.deliver()
      |> case do
        {:ok, _} -> :ok
        {:error, why} -> Logger.warning("watchdog: the alert didn't send: #{inspect(why)}")
      end
    end
  end

  defp schedule do
    case Application.get_env(:presence_relay, :watchdog_ms, 60_000) do
      ms when is_integer(ms) -> Process.send_after(self(), :check, ms)
      _ -> nil
    end
  end
end
