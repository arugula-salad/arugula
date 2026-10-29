defmodule PresenceRelay.ClusterTest do
  # Two nodes, one room (#318's P2): members connected to different nodes
  # see each other's frames, waves, reactions, presence and `off`, and the
  # caps hold across the cluster.
  use PresenceRelay.RelayCase, async: false

  alias PresenceRelay.Cluster

  @moduletag :cluster
  @frame <<0, 0x40, 1, 2, 3>>
  # Presence replicates on Phoenix.Tracker's broadcast period.
  @slow 4_000

  setup_all do
    {peer, node} = Cluster.start_peer!()
    on_exit(fn -> :peer.stop(peer) end)
    %{peer: node}
  end

  test "frames, waves, reactions and presence cross the nodes", %{peer: peer} do
    r = room()
    {:ok, a, _} = client(:a, r, "alice")
    {:ok, b, _} = client_on(peer, :b, r, "bob")

    send_as(b, "frame", {:binary, @frame})
    expected = framed("bob", @frame)
    assert_receive {:pushed, :a, "frame", ^expected}, 1_000

    send_as(a, "frame", {:binary, @frame})
    expected = framed("alice", @frame)
    assert_receive {:pushed, :b, "frame", ^expected}, 1_000

    send_as(a, "wave", %{"to" => "bob"})
    assert_receive {:pushed, :b, "wave", %{from: "alice"}}, 1_000
    send_as(b, "answer", %{"to" => "alice", "yes" => true})
    assert_receive {:pushed, :a, "answer", %{from: "bob", yes: true}}, 1_000

    send_as(b, "react", %{"emoji" => "🎉"})
    assert_receive {:pushed, :a, "react", %{from: "bob", emoji: "🎉"}}, 1_000

    send_as(b, "state", %{"mode" => "avatar"})

    assert_receive {:pushed, :a, "presence_diff",
                    %{joins: %{"bob" => %{metas: [%{mode: "avatar"}]}}}},
                   @slow
  end

  test "a joiner gets the newest frames cached on the other node", %{peer: peer} do
    r = room()
    {:ok, b, _} = client_on(peer, :b, r, "bob")
    send_as(b, "frame", {:binary, @frame})
    Process.sleep(50)
    {:ok, _late, _} = client(:late, r, "zed")
    expected = framed("bob", @frame)
    assert_receive {:pushed, :late, "frame", ^expected}, 1_000
  end

  test "off crosses the nodes: a camera off, a stale frame, a last connection anywhere", %{
    peer: peer
  } do
    r = room()
    {:ok, _a, _} = client(:a, r, "alice")
    {:ok, b, _} = client_on(peer, :b, r, "bob")

    # Camera off.
    send_as(b, "frame", {:binary, @frame})
    send_as(b, "state", %{"mode" => "none"})
    assert_receive {:pushed, :a, "off", %{sub: "bob"}}, 1_000

    # Stale: swept on the node that holds the frame (stale_ms is 300 in test).
    send_as(b, "frame", {:binary, <<9>>})
    assert_receive {:pushed, :a, "frame", _}, 1_000
    assert_receive {:pushed, :a, "off", %{sub: "bob"}}, 1_500

    # Bob on both nodes: leaving one isn't leaving.
    {:ok, b2, _} = client(:b2, r, "bob")
    leave(b, :b)
    refute_receive {:pushed, :a, "off", %{sub: "bob"}}, 300
    leave(b2, :b2)
    assert_receive {:pushed, :a, "off", %{sub: "bob"}}, 1_000
  end

  test "the caps count the whole cluster", %{peer: peer} do
    Cluster.put_env(peer, :max_members, 2)
    Cluster.put_env(peer, :max_conns, 2)

    on_exit(fn ->
      Cluster.put_env(peer, :max_members, 64)
      Cluster.put_env(peer, :max_conns, 8)
    end)

    r = room()
    {:ok, _, _} = client(:a, r, "alice")
    {:ok, _, _} = client_on(peer, :b, r, "bob")
    Process.sleep(50)
    assert {:error, %{reason: "room full"}} = client_on(peer, :c, r, "carol")
    assert {:error, %{reason: "room full"}} = client(:c2, r, "carol")

    # Alice once here, once there: two connections, then no more.
    {:ok, _, _} = client_on(peer, :a2, r, "alice")
    Process.sleep(50)
    assert {:error, %{reason: "too many connections"}} = client(:a3, r, "alice")
  end
end
