defmodule PresenceRelay.RoomTest do
  use PresenceRelay.RelayCase, async: false

  @frame <<0, 0x40, 1, 2, 3>>

  test "joining replies with who you are, and presence shows you" do
    r = room()
    assert {:ok, _a, %{you: "a"}} = client(:a, r, "a", %{"name" => "Ada"})
    assert_receive {:pushed, :a, "presence_state", %{"a" => %{metas: [meta]}}}
    assert %{name: "Ada", mode: "none", away: false, watching: true} = meta
  end

  test "a frame goes to everyone else, prefixed with its sender, and not back to it" do
    r = room()
    {:ok, a, _} = client(:a, r, "alice")
    {:ok, _b, _} = client(:b, r, "bob")
    {:ok, _c, _} = client(:c, r, "carol")
    send_as(a, "frame", {:binary, @frame})
    expected = framed("alice", @frame)
    assert_receive {:pushed, :b, "frame", ^expected}
    assert_receive {:pushed, :c, "frame", ^expected}
    refute_receive {:pushed, :a, "frame", _}, 150
  end

  test "a joiner gets every member's newest frame" do
    r = room()
    {:ok, a, _} = client(:a, r, "alice")
    send_as(a, "frame", {:binary, @frame})
    Process.sleep(30)
    {:ok, _late, _} = client(:late, r, "zed")
    expected = framed("alice", @frame)
    assert_receive {:pushed, :late, "frame", ^expected}
  end

  test "frames closer than 70 ms and frames over 32 KiB are dropped without a word" do
    r = room()
    {:ok, a, _} = client(:a, r, "alice")
    {:ok, _b, _} = client(:b, r, "bob")
    send_as(a, "frame", {:binary, <<1>>})
    send_as(a, "frame", {:binary, <<2>>})
    assert_receive {:pushed, :b, "frame", {:binary, <<5, "alice", 1>>}}
    refute_receive {:pushed, :b, "frame", {:binary, <<5, "alice", 2>>}}, 100
    send_as(a, "frame", {:binary, :binary.copy(<<7>>, 32 * 1024 + 1)})
    refute_receive {:pushed, :b, "frame", _}, 100
    big = :binary.copy(<<8>>, 32 * 1024)
    send_as(a, "frame", {:binary, big})
    assert_receive {:pushed, :b, "frame", {:binary, <<5, "alice", ^big::binary>>}}
    # A frame that isn't binary is no frame.
    Process.sleep(80)
    send_as(a, "frame", %{"not" => "binary"})
    refute_receive {:pushed, :b, "frame", _}, 100
  end

  test "state updates the member's presence; only the known keys, checked" do
    r = room()
    {:ok, a, _} = client(:a, r, "alice")
    {:ok, _b, _} = client(:b, r, "bob")
    assert_receive {:pushed, :b, "presence_state", _}

    send_as(a, "state", %{
      "mode" => "avatar",
      "away" => true,
      "look" => %{"character" => 3, "colour" => 2},
      "evil" => 1,
      "watching" => "yes"
    })

    assert_receive {:pushed, :b, "presence_diff", %{joins: %{"alice" => %{metas: [meta]}}}}

    assert %{mode: "avatar", away: true, look: %{"character" => 3, "colour" => 2}, watching: true} =
             meta

    refute Map.has_key?(meta, :evil)
  end

  test "a camera going off, a stale frame and a last connection leaving each say off" do
    r = room()
    {:ok, a, _} = client(:a, r, "alice")
    {:ok, _b, _} = client(:b, r, "bob")
    send_as(a, "state", %{"mode" => "none"})
    assert_receive {:pushed, :b, "off", %{sub: "alice"}}
    # Stale: test config makes a frame stale at 300 ms, swept every 50.
    send_as(a, "frame", {:binary, @frame})
    assert_receive {:pushed, :b, "frame", _}
    assert_receive {:pushed, :b, "off", %{sub: "alice"}}, 1000
    leave(a, :a)
    assert_receive {:pushed, :b, "off", %{sub: "alice"}}
  end

  test "a member with two connections is still there when one leaves" do
    r = room()
    {:ok, a1, _} = client(:a1, r, "alice")
    {:ok, _a2, _} = client(:a2, r, "alice")
    {:ok, _b, _} = client(:b, r, "bob")
    leave(a1, :a1)
    refute_receive {:pushed, :b, "off", %{sub: "alice"}}, 150
  end

  test "waves and answers reach only the member they're for" do
    r = room()
    {:ok, a, _} = client(:a, r, "alice")
    {:ok, b, _} = client(:b, r, "bob")
    {:ok, _c, _} = client(:c, r, "carol")
    send_as(a, "wave", %{"to" => "bob"})
    assert_receive {:pushed, :b, "wave", %{from: "alice"}}
    refute_receive {:pushed, :c, "wave", _}, 100
    refute_receive {:pushed, :a, "wave", _}, 10
    send_as(b, "answer", %{"to" => "alice", "yes" => true})
    assert_receive {:pushed, :a, "answer", %{from: "bob", yes: true}}
    refute_receive {:pushed, :c, "answer", _}, 100
  end

  test "two apps' rooms of one name never meet" do
    r = room()
    {:ok, a, _} = client(:a, r, "alice")
    test = self()

    spawn(fn ->
      tok = token(%{"room" => r, "sub" => "eve", "name" => "Eve"}, "other")

      {:ok, s} =
        Phoenix.ChannelTest.__connect__(@endpoint, PresenceRelayWeb.Socket, %{"token" => tok},
          test_process: test
        )

      {:ok, _, _} = join(s, PresenceRelayWeb.RoomChannel, "room:" <> r, %{})
      send(test, :eve_in)

      receive do
        %Phoenix.Socket.Message{event: "frame"} -> send(test, :eve_saw_frame)
      after
        300 -> send(test, :eve_saw_nothing)
      end
    end)

    assert_receive :eve_in
    send_as(a, "frame", {:binary, @frame})
    assert_receive :eve_saw_nothing, 1000
  end

  test "a member holds 8 connections, and a room 64 members" do
    r = room()
    for i <- 1..8, do: assert({:ok, _, _} = client(:"a#{i}", r, "alice"))
    assert {:error, %{reason: "too many connections"}} = client(:a9, r, "alice")
    Application.put_env(:presence_relay, :max_members, 3)

    try do
      assert {:ok, _, _} = client(:b, r, "bob")
      assert {:ok, _, _} = client(:c, r, "carol")
      assert {:error, %{reason: "room full"}} = client(:d, r, "dave")
      # A member already in comes in on another connection.
      assert {:ok, _, _} = client(:b2, r, "bob")
    after
      Application.delete_env(:presence_relay, :max_members)
    end
  end

  test "a reaction goes to the rest of the room; text, two emoji and a flood are dropped" do
    r = room()
    {:ok, a, _} = client(:a, r, "alice")
    {:ok, _b, _} = client(:b, r, "bob")
    send_as(a, "react", %{"emoji" => "👍🏽"})
    assert_receive {:pushed, :b, "react", %{from: "alice", emoji: "👍🏽"}}
    refute_receive {:pushed, :a, "react", _}, 50
    # Within 250 ms of the last: dropped.
    send_as(a, "react", %{"emoji" => "🎉"})
    refute_receive {:pushed, :b, "react", %{emoji: "🎉"}}, 100
    Process.sleep(260)

    for bad <- ["hi", "👍👍", "a👍", String.duplicate("x", 40), 7] do
      send_as(a, "react", %{"emoji" => bad})
    end

    refute_receive {:pushed, :b, "react", _}, 100
    Process.sleep(260)
    send_as(a, "react", %{"emoji" => "❤️"})
    assert_receive {:pushed, :b, "react", %{from: "alice", emoji: "❤️"}}
  end
end
