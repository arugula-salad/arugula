defmodule PresenceRelay.ChatTest do
  use PresenceRelay.RelayCase, async: false

  alias PresenceRelay.Chat

  describe "check/1" do
    test "trims, and refuses empty, too long and control characters (a newline is fine)" do
      assert Chat.check("  hi  ") == {:ok, "hi"}
      assert Chat.check("line one\nline two") == {:ok, "line one\nline two"}
      assert Chat.check("   ") == {:error, :empty}
      assert Chat.check(String.duplicate("é", 2001)) == {:error, :too_long}
      assert {:ok, _} = Chat.check(String.duplicate("é", 2000))
      assert Chat.check("bell\a") == {:error, :control}
      assert Chat.check("tab\there") == {:error, :control}
      assert Chat.check(7) == {:error, :empty}
    end
  end

  for store <- [:postgres, :memory] do
    describe "kept in #{store}" do
      setup do
        prev = Application.get_env(:presence_relay, :database)
        Application.put_env(:presence_relay, :database, unquote(store) == :postgres)
        on_exit(fn -> Application.put_env(:presence_relay, :database, prev) end)
        :ok
      end

      test "messages come back oldest first, per app and room, with ids that go up" do
        r = room()
        {:ok, a} = Chat.say("acme", r, "alice", "Alice", "one")
        {:ok, b} = Chat.say("acme", r, "bob", "Bob", "two")
        {:ok, _} = Chat.say("other", r, "zed", "Zed", "elsewhere")
        assert b.id > a.id

        assert [%{text: "one", sub: "alice", name: "Alice"}, %{text: "two"}] =
                 Chat.history("acme", r)

        assert is_integer(a.at)
      end

      test "history is the last page, and before: pages back" do
        r = room()
        msgs = for i <- 1..55, do: elem(Chat.say("acme", r, "a", "A", "m#{i}"), 1)
        last = Chat.history("acme", r)
        assert length(last) == 50
        assert hd(last).text == "m6" and List.last(last).text == "m55"
        earlier = Chat.history("acme", r, hd(last).id)

        # Postgres keeps everything; memory keeps a room's last 50.
        if unquote(store) == :postgres,
          do: assert(Enum.map(earlier, & &1.text) == ["m1", "m2", "m3", "m4", "m5"]),
          else: assert(earlier == [])

        assert Chat.history("acme", r, hd(msgs).id) == []
      end
    end
  end

  describe "in a room" do
    test "say reaches everyone else as said, and the sender as a reply with the id" do
      r = room()
      {:ok, a, _} = client(:a, r, "alice", %{"name" => "Alice"})
      {:ok, _b, _} = client(:b, r, "bob")
      send_as(a, "say", %{"text" => " hello room "})

      assert_receive {:replied, :a, :ok,
                      %{id: id, text: "hello room", sub: "alice", name: "Alice"}}

      assert_receive {:pushed, :b, "said", %{id: ^id, text: "hello room"}}
      refute_receive {:pushed, :a, "said", _}, 100
    end

    test "a joiner gets the history, and history before: an id pages back" do
      r = room()
      {:ok, a, _} = client(:a, r, "alice")
      for t <- ~w(first second), do: send_as(a, "say", %{"text" => t})
      assert_receive {:replied, :a, :ok, %{text: "second", id: second}}
      {:ok, late, _} = client(:late, r, "zed")

      assert_receive {:pushed, :late, "history",
                      %{messages: [%{text: "first"}, %{text: "second"}]}}

      send_as(late, "history", %{"before" => second})
      assert_receive {:replied, :late, :ok, %{messages: [%{text: "first"}]}}
    end

    test "bad text and floods get an error reply and reach nobody" do
      r = room()
      {:ok, a, _} = client(:a, r, "alice")
      {:ok, _b, _} = client(:b, r, "bob")
      send_as(a, "say", %{"text" => "  "})
      assert_receive {:replied, :a, :error, %{reason: "empty"}}
      send_as(a, "say", %{"text" => String.duplicate("x", 2001)})
      assert_receive {:replied, :a, :error, %{reason: "too long"}}

      for i <- 1..6, do: send_as(a, "say", %{"text" => "msg #{i}"})
      for _ <- 1..5, do: assert_receive({:replied, :a, :ok, _})
      assert_receive {:replied, :a, :error, %{reason: "too fast"}}
      refute_receive {:pushed, :b, "said", %{text: "msg 6"}}, 100
    end
  end
end
