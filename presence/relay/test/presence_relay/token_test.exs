defmodule PresenceRelay.TokenTest do
  use PresenceRelay.RelayCase, async: false

  alias PresenceRelay.Token

  test "a token signed with the app's secret admits a socket, carrying its claims" do
    assert {:ok, socket} =
             connect(PresenceRelayWeb.Socket, %{
               "token" => token(%{"sub" => "jake", "name" => "Jake"})
             })

    assert %{app: "acme", room: "lobby", sub: "jake", name: "Jake"} = socket.assigns
  end

  test "a bad signature, another app's secret, an unknown app, or no token is refused" do
    good = token()
    [h, p, _sig] = String.split(good, ".")
    assert :error = connect(PresenceRelayWeb.Socket, %{"token" => h <> "." <> p <> ".AAAA"})
    # Signed with "other"'s secret but claiming app "acme".
    forged =
      Token.sign("other", %{
        "room" => "lobby",
        "sub" => "x",
        "name" => "X",
        "exp" => System.system_time(:second) + 60
      })

    [fh, _fp, fsig] = String.split(forged, ".")
    [_, acme_payload, _] = String.split(token(%{"sub" => "x"}), ".")

    assert :error =
             connect(PresenceRelayWeb.Socket, %{
               "token" => fh <> "." <> acme_payload <> "." <> fsig
             })

    assert {:error, :unknown_app} =
             Token.verify(
               h <> "." <> Base.url_encode64(~s({"app":"nope"}), padding: false) <> ".x"
             )

    assert :error = connect(PresenceRelayWeb.Socket, %{})
    assert :error = connect(PresenceRelayWeb.Socket, %{"token" => "not.a.jwt"})
  end

  test "an expired token, one living over a day, or bad claims are refused" do
    now = System.system_time(:second)
    assert {:error, :expired} = Token.verify(token(%{"exp" => now - 1}))
    assert {:error, :lives_too_long} = Token.verify(token(%{"exp" => now + 3 * 24 * 3600}))
    assert {:error, :bad_room} = Token.verify(token(%{"room" => "no spaces"}))
    assert {:error, :bad_sub} = Token.verify(token(%{"sub" => ""}))
    assert {:error, :bad_name} = Token.verify(token(%{"name" => String.duplicate("x", 81)}))

    assert {:ok, %{look: %{"character" => 2, "colour" => 1}}} =
             Token.verify(token(%{"look" => %{"character" => 2, "colour" => 1}}))
  end

  test "a token joins its own room only" do
    {:ok, socket} = connect(PresenceRelayWeb.Socket, %{"token" => token(%{"room" => "mine"})})

    assert {:error, %{reason: "not this token's room"}} =
             join(socket, PresenceRelayWeb.RoomChannel, "room:yours", %{})

    assert {:ok, %{you: _}, _} = join(socket, PresenceRelayWeb.RoomChannel, "room:mine", %{})
  end
end
