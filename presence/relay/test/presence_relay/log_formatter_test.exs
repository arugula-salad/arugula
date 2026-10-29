defmodule PresenceRelay.LogFormatterTest do
  use ExUnit.Case, async: true

  alias PresenceRelay.LogFormatter

  @jwt "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJ1MSJ9.c2lnbmF0dXJlLWJ5dGVz"

  # Key-shaped, made up, and split so secret scanners don't take it for one.
  @live_key "pk_" <> "live_" <> "0123456789abcdefgh"

  test "tokens, addresses, keys and passwords are blanked" do
    cases = [
      {"sent a link to ada@example.com", "ada@example.com"},
      {"GET /auth/email/AbC123_-xyz", "AbC123_-xyz"},
      {"verify failed for #{@jwt}", @jwt},
      {"/socket/websocket?token=abc.def.ghi&vsn=2.0.0", "abc.def.ghi"},
      {"key=#{@live_key} room=x", @live_key},
      {"publishable pk_test_ZZZZZZZZZZZZ", "pk_test_ZZZZZZZZZZZZ"},
      {"Authorization: Bearer s3cr3t-value", "s3cr3t-value"},
      {"postgres://presence:hunter2@db.internal:5432/x", "hunter2"},
      {"resend re_123456789012345678901234", "re_123456789012345678901234"},
      {"auth=whatever&name=Ada", "whatever"}
    ]

    for {line, secret} <- cases do
      out = LogFormatter.scrub(line)
      refute out =~ secret, "#{line} → #{out}"
    end

    # What isn't a secret stays.
    assert LogFormatter.scrub("JOINED room:standup in 120µs") == "JOINED room:standup in 120µs"
    assert LogFormatter.scrub("&name=Ada") == "&name=Ada"
  end

  test "a line is one JSON object: time, level, message, source, request id and app only" do
    event = %{
      level: :info,
      msg: {:string, "link for bob@example.com, token=#{@jwt}"},
      meta: %{
        time: System.os_time(:microsecond),
        mfa: {PresenceRelayWeb.AuthController, :email_request, 2},
        request_id: "F-abc123",
        app: "app_1",
        email: "bob@example.com",
        token: @jwt,
        pid: self()
      }
    }

    line = event |> LogFormatter.format(%{}) |> IO.iodata_to_binary()
    assert String.ends_with?(line, "\n")
    refute line =~ "bob@example.com"
    refute line =~ @jwt

    assert %{
             "time" => _,
             "level" => "info",
             "msg" => "link for [email], token=[filtered]",
             "source" => "PresenceRelayWeb.AuthController.email_request/2",
             "request_id" => "F-abc123",
             "app" => "app_1"
           } = decoded = Jason.decode!(line)

    assert Map.keys(decoded) |> Enum.sort() == ~w(app level msg request_id source time)
  end

  test "a report or a format string is formatted, and scrubbed, too" do
    for msg <- [{"sent to ~s", ["cy@example.com"]}, {:report, %{to: "cy@example.com"}}] do
      line =
        %{level: :warning, msg: msg, meta: %{}}
        |> LogFormatter.format(%{})
        |> IO.iodata_to_binary()

      assert %{"level" => "warning", "msg" => m} = Jason.decode!(line)
      refute m =~ "cy@example.com"
      assert m =~ "[email]"
    end
  end
end
