defmodule PresenceRelay.CallTest do
  use PresenceRelay.RelayCase, async: false

  # Calls (PROTOCOL.md, "Calls"): ringing one member, a yes giving both
  # connections a LiveKit token for one room, a no or no answer giving
  # nothing, and the refusals. Test config has LiveKit keys, so calls are on.

  @secret "test-livekit-secret"

  defp claims(token) do
    [h, p, sig] = String.split(token, ".")
    expected = :crypto.mac(:hmac, :sha256, @secret, h <> "." <> p)
    assert Base.url_decode64!(sig, padding: false) == expected, "signed with the API secret"

    assert %{"alg" => "HS256", "typ" => "JWT"} =
             Jason.decode!(Base.url_decode64!(h, padding: false))

    Jason.decode!(Base.url_decode64!(p, padding: false))
  end

  defp ring(caller, callee_name, to) do
    send_as(caller, "call", %{"to" => to})
    assert_receive {:replied, _, :ok, %{id: id}}
    assert_receive {:pushed, ^callee_name, "call", %{from: _, id: ^id}}
    id
  end

  test "the join reply says calls are on, and presence carries who's in a call" do
    r = room()
    assert {:ok, _a, %{you: "alice", calls: true}} = client(:a, r, "alice")
    assert_receive {:pushed, :a, "presence_state", %{"alice" => %{metas: [%{call: nil}]}}}
  end

  test "a call's token publishes the microphone and the camera (P8's video), never a screen or data" do
    t = PresenceRelay.Calls.join_info("acme", "r", "c1", "alice", "Alice").token
    grant = claims(t)["video"]
    assert grant["canPublishSources"] == ["microphone", "camera"]
    refute "screen_share" in grant["canPublishSources"]
    assert grant["canPublishData"] == false
    assert grant["room"] == "acme:r:c1"
  end

  test "a yes gives both connections a token for one LiveKit room, scoped by app, room and call" do
    r = room()
    {:ok, a, _} = client(:a, r, "alice", %{"name" => "Alice"})
    {:ok, b, _} = client(:b, r, "bob", %{"name" => "Bob"})
    {:ok, _c, _} = client(:c, r, "carol")
    id = ring(a, :b, "bob")
    refute_receive {:pushed, :c, "call", _}, 50

    send_as(b, "call_answer", %{"id" => id, "yes" => true})
    assert_receive {:replied, :b, :ok, _}
    assert_receive {:pushed, :a, "call_answer", %{id: ^id, from: "bob", yes: true}}
    assert_receive {:pushed, :a, "call_join", %{id: ^id, url: "wss://livekit.test"} = ja}
    assert_receive {:pushed, :b, "call_join", %{id: ^id, url: "wss://livekit.test"} = jb}
    refute_receive {:pushed, :c, "call_join", _}, 50

    lk_room = "acme:#{r}:#{id}"
    assert ja.room == lk_room and jb.room == lk_room
    ca = claims(ja.token)
    cb = claims(jb.token)
    assert %{"iss" => "test-key", "sub" => "alice", "name" => "Alice"} = ca
    assert %{"sub" => "bob", "name" => "Bob"} = cb

    assert ca["video"] == %{
             "room" => lk_room,
             "roomJoin" => true,
             "canPublish" => true,
             "canPublishSources" => ["microphone", "camera"],
             "canPublishData" => false,
             "canSubscribe" => true
           }

    now = System.system_time(:second)
    assert (ca["exp"] - now) in 590..600, "lives ten minutes"
    assert ca["nbf"] <= now

    # Presence says both are in the call, for tiles.
    assert_receive {:pushed, :c, "presence_diff",
                    %{joins: %{"alice" => %{metas: [%{call: ^id}]}}}}

    assert_receive {:pushed, :c, "presence_diff", %{joins: %{"bob" => %{metas: [%{call: ^id}]}}}}
    assert PresenceRelay.Calls.size("acme", r, id) == 2

    # A fresh token, for a page that dropped out; only for its own call.
    send_as(a, "call_token", %{"id" => id})
    assert_receive {:replied, :a, :ok, %{token: t, room: ^lk_room}}
    assert %{"sub" => "alice"} = claims(t)
    send_as(a, "call_token", %{"id" => "someone-elses"})
    assert_receive {:replied, :a, :error, %{reason: "not in that call"}}

    # Leaving: presence says so.
    send_as(a, "call_leave", %{"id" => id})
    assert_receive {:replied, :a, :ok, _}

    assert_receive {:pushed, :c, "presence_diff",
                    %{joins: %{"alice" => %{metas: [%{call: nil}]}}}}

    assert PresenceRelay.Calls.size("acme", r, id) == 1
  end

  test "a no gives nobody a token, and the caller hears it" do
    r = room()
    {:ok, a, _} = client(:a, r, "alice")
    {:ok, b, _} = client(:b, r, "bob")
    id = ring(a, :b, "bob")
    send_as(b, "call_answer", %{"id" => id, "yes" => false})
    assert_receive {:pushed, :a, "call_answer", %{id: ^id, from: "bob", yes: false}}
    refute_receive {:pushed, _, "call_join", _}, 100
    # Answered once: a second answer has no call.
    send_as(b, "call_answer", %{"id" => id, "yes" => true})
    assert_receive {:replied, :b, :error, %{reason: "no such call"}}
  end

  test "only a connection that was rung may answer, and the callee's other pages stop ringing" do
    r = room()
    {:ok, a, _} = client(:a, r, "alice")
    {:ok, b1, _} = client(:b1, r, "bob")
    {:ok, _b2, _} = client(:b2, r, "bob")
    {:ok, c, _} = client(:c, r, "carol")
    id = ring(a, :b1, "bob")
    assert_receive {:pushed, :b2, "call", %{id: ^id}}
    # Carol wasn't rung: she can't answer her way in.
    send_as(c, "call_answer", %{"id" => id, "yes" => true})
    assert_receive {:replied, :c, :error, %{reason: "no such call"}}
    send_as(b1, "call_answer", %{"id" => id, "yes" => true})
    assert_receive {:pushed, :b2, "call_cancel", %{id: ^id}}
    assert_receive {:pushed, :b1, "call_join", _}
    refute_receive {:pushed, :b2, "call_join", _}, 100
  end

  test "a ring nobody answers ends, and a caller can give up first" do
    r = room()
    {:ok, a, _} = client(:a, r, "alice")
    {:ok, b, _} = client(:b, r, "bob")
    id = ring(a, :b, "bob")
    # Test config gives up after 300 ms.
    assert_receive {:pushed, :b, "call_cancel", %{id: ^id}}, 1000

    assert_receive {:pushed, :a, "call_answer",
                    %{id: ^id, from: "bob", yes: false, reason: "no answer"}}

    send_as(b, "call_answer", %{"id" => id, "yes" => true})
    assert_receive {:replied, :b, :error, %{reason: "no such call"}}

    id2 = ring(a, :b, "bob")
    send_as(a, "call_leave", %{"id" => id2})
    assert_receive {:pushed, :b, "call_cancel", %{id: ^id2}}
  end

  test "calling yourself, someone not in the room, or too often is refused" do
    r = room()
    {:ok, a, _} = client(:a, r, "alice")
    {:ok, _b, _} = client(:b, r, "bob")
    send_as(a, "call", %{"to" => "alice"})
    assert_receive {:replied, :a, :error, %{reason: "that's you"}}
    send_as(a, "call", %{"to" => "nobody"})
    assert_receive {:replied, :a, :error, %{reason: "not in the room"}}
    # Someone in another room of the app isn't in this one.
    {:ok, _z, _} = client(:z, room(), "zed")
    send_as(a, "call", %{"to" => "zed"})
    assert_receive {:replied, :a, :error, %{reason: "not in the room"}}

    for _ <- 1..3, do: ring(a, :b, "bob")
    send_as(a, "call", %{"to" => "bob"})
    assert_receive {:replied, :a, :error, %{reason: "too fast"}}
    refute_receive {:pushed, :b, "call", _}, 50
  end

  test "a caller in a call rings someone into it: the call grows" do
    r = room()
    {:ok, a, _} = client(:a, r, "alice")
    {:ok, b, _} = client(:b, r, "bob")
    {:ok, c, _} = client(:c, r, "carol")
    id = ring(a, :b, "bob")
    send_as(b, "call_answer", %{"id" => id, "yes" => true})
    assert_receive {:pushed, :b, "call_join", _}
    assert_receive {:pushed, :a, "call_join", _}
    # Bob is in it already.
    Process.sleep(50)
    send_as(a, "call", %{"to" => "bob"})
    assert_receive {:replied, :a, :error, %{reason: "already in the call"}}
    ^id = ring(a, :c, "carol")
    send_as(c, "call_answer", %{"id" => id, "yes" => true})
    assert_receive {:pushed, :c, "call_join", %{id: ^id}}
    refute_receive {:pushed, :a, "call_join", _}, 100
    assert PresenceRelay.Calls.size("acme", r, id) == 3
  end

  test "start, join, leave and stop are told as telemetry (the metering hook)" do
    r = room()
    test = self()
    handler = "call-test-#{r}"

    :telemetry.attach_many(
      handler,
      for(k <- [:start, :join, :leave, :stop], do: [:presence_relay, :call, k]),
      fn [_, _, kind], m, meta, _ -> send(test, {:call_event, kind, m, meta}) end,
      nil
    )

    on_exit(fn -> :telemetry.detach(handler) end)
    {:ok, a, _} = client(:a, r, "alice")
    {:ok, b, _} = client(:b, r, "bob")
    id = ring(a, :b, "bob")
    send_as(b, "call_answer", %{"id" => id, "yes" => true})
    assert_receive {:call_event, :start, _, %{app: "acme", room: ^r, call: ^id}}
    assert_receive {:call_event, :join, _, %{call: ^id, sub: "alice"}}
    assert_receive {:call_event, :join, _, %{call: ^id, sub: "bob"}}
    send_as(a, "call_leave", %{"id" => id})
    assert_receive {:call_event, :leave, %{duration_ms: ms}, %{call: ^id, sub: "alice"}}
    assert is_integer(ms) and ms >= 0
    refute_receive {:call_event, :stop, _, _}, 50
    # Bob's page closing ends it.
    leave(b, :b)
    assert_receive {:call_event, :leave, _, %{call: ^id, sub: "bob"}}
    assert_receive {:call_event, :stop, _, %{call: ^id}}
    assert PresenceRelay.Calls.size("acme", r, id) == 0
  end

  test "without LiveKit keys calls are off: the join reply says so and call is refused" do
    was = Application.get_env(:presence_relay, :livekit)
    Application.delete_env(:presence_relay, :livekit)
    on_exit(fn -> Application.put_env(:presence_relay, :livekit, was) end)
    r = room()
    assert {:ok, a, %{calls: false}} = client(:a, r, "alice")
    {:ok, _b, _} = client(:b, r, "bob")
    send_as(a, "call", %{"to" => "bob"})
    assert_receive {:replied, :a, :error, %{reason: "calls aren't set up here"}}
    refute_receive {:pushed, :b, "call", _}, 50
    # Everything else carries on.
    send_as(a, "wave", %{"to" => "bob"})
    assert_receive {:pushed, :b, "wave", %{from: "alice"}}
  end
end
