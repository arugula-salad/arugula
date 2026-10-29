defmodule PresenceRelay.MetricsTest do
  use PresenceRelay.DataCase, async: false
  import Plug.Conn
  import Phoenix.ConnTest
  import Swoosh.TestAssertions
  import PresenceRelay.RelayCase, only: [client: 4, room: 0, send_as: 3]

  alias PresenceRelay.{Metrics, Watchdog}

  @endpoint PresenceRelayWeb.Endpoint

  defp text, do: Metrics.render() |> IO.iodata_to_binary()

  # A series' value (`name` with its labels exactly as written), or nil.
  defp value(text, series) do
    Enum.find_value(String.split(text, "\n"), fn line ->
      case String.split(line, " ") do
        [^series, v] -> elem(Float.parse(v), 0)
        _ -> nil
      end
    end)
  end

  defp put_env(key, value) do
    before = Application.get_env(:presence_relay, key)
    Application.put_env(:presence_relay, key, value)
    on_exit(fn -> Application.put_env(:presence_relay, key, before) end)
  end

  test "every line is a comment or a sample, and every metric says its type and help" do
    t = text()

    for line <- String.split(t, "\n", trim: true) do
      assert line =~
               ~r/\A(# (HELP|TYPE) [a-z_]+ .+|[a-z_]+(\{[a-z_]+="[^"]*"(,[a-z_]+="[^"]*")*\})? -?[0-9.e+-]+|[a-z_]+(\{.*\})? \+?Inf)\z/,
             line
    end

    for name <-
          ~w(presence_connections presence_rooms presence_members presence_calls_active
             presence_call_participants presence_cluster_nodes presence_usage_unflushed_seconds
             presence_db_pool_size presence_beam_memory_bytes presence_beam_processes
             presence_beam_run_queue presence_frames_total presence_joins_refused_total
             presence_db_query_seconds presence_http_request_seconds) do
      assert t =~ "# TYPE #{name} ", name
      assert t =~ "# HELP #{name} ", name
    end
  end

  test "rooms, members and frames are counted as they happen" do
    r = room()
    before = text()
    {:ok, a, _} = client(:a, r, "u_a", [])
    {:ok, _b, _} = client(:b, r, "u_b", [])

    send_as(a, "frame", {:binary, <<0, 0x40, 1, 2, 3>>})
    assert_receive {:pushed, :b, "frame", _}
    send_as(a, "frame", {:binary, <<0, 0x40, 1, 2, 3>>})
    send_as(a, "react", %{"emoji" => "🎉"})
    assert_receive {:pushed, :b, "react", _}

    after_ = text()
    delta = &((value(after_, &1) || 0) - (value(before, &1) || 0))

    assert delta.("presence_joins_total") == 2
    assert delta.(~s(presence_frames_total{result="relayed"})) == 1
    assert delta.(~s(presence_frames_total{result="too_soon"})) == 1
    assert delta.("presence_frame_bytes_total") == 5
    assert delta.(~s(presence_messages_total{kind="react",result="ok"})) == 1
    assert value(after_, "presence_rooms") >= 1
    assert value(after_, "presence_members") >= 2
    assert value(after_, "presence_connections") >= 2
  end

  test "refusals are counted by reason" do
    before = text()
    r = room()
    assert {:error, _} = client(:x, r, "u_x", room: "elsewhere")
    assert :error = PresenceRelayWeb.Socket.connect(%{"token" => "nope"}, %Phoenix.Socket{}, %{})
    t = text()

    assert value(t, ~s(presence_joins_refused_total{reason="wrong_room"})) >
             (value(before, ~s(presence_joins_refused_total{reason="wrong_room"})) || 0)

    assert value(t, ~s(presence_joins_refused_total{reason="token"})) >
             (value(before, ~s(presence_joins_refused_total{reason="token"})) || 0)
  end

  test "histograms' buckets add up, and calls' seconds are seconds" do
    Metrics.observe(:presence_usage_flush_seconds, [], 0.02)
    Metrics.observe(:presence_usage_flush_seconds, [], 100)
    :telemetry.execute([:presence_relay, :call, :leave], %{duration_ms: 1500}, %{app: "acme"})
    t = text()

    count = value(t, "presence_usage_flush_seconds_count")
    assert value(t, ~s(presence_usage_flush_seconds_bucket{le="+Inf"})) == count
    assert value(t, ~s(presence_usage_flush_seconds_bucket{le="0.05"})) >= 1
    assert value(t, "presence_usage_flush_seconds_sum") >= 100.02
    assert value(t, "presence_call_seconds_total") >= 1.5
  end

  test "the internal port serves /metrics and nothing else" do
    conn = PresenceRelayWeb.MetricsPlug.call(Plug.Test.conn(:get, "/metrics"), [])
    assert conn.status == 200
    assert [ct] = get_resp_header(conn, "content-type")
    assert ct =~ "text/plain; version=0.0.4"
    assert conn.resp_body =~ "# TYPE presence_connections gauge"

    assert PresenceRelayWeb.MetricsPlug.call(Plug.Test.conn(:get, "/"), []).status == 404
  end

  test "healthz?deep=1: every check, as JSON" do
    conn = get(build_conn(), "/healthz?deep=1")
    assert %{"ok" => true, "checks" => %{"db" => %{"ok" => true}}} = json_response(conn, 200)
  end

  @tag :capture_log
  test "the watchdog mails after three failing checks, once, and again when well" do
    Swoosh.TestAssertions.set_swoosh_global()
    # Usage that's never flushed clean counts as stuck.
    put_env(:usage_flush_ms, 5_000)
    put_env(:usage_stuck_s, 0)

    assert json_response(get(build_conn(), "/healthz?deep=1"), 503)["checks"]["usage"]["ok"] ==
             false

    Watchdog.check()
    Watchdog.check()
    refute_email_sent()
    Watchdog.check()

    assert_email_sent(fn e ->
      assert e.to == [{"", "hello@arugula.stream"}]
      assert e.subject =~ "health failing"
      assert e.text_body =~ "usage"
    end)

    Watchdog.check()
    refute_email_sent()

    Application.put_env(:presence_relay, :usage_stuck_s, 1_000_000)
    Watchdog.check()
    assert_email_sent(subject: "arugula.stream relay: well again")
  end
end
