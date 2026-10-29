defmodule PresenceRelay.VectorsTest do
  # The server SDKs' test vectors (../sdk/vectors.json) against the relay:
  # each vector token is signed the way the relay checks (its test app
  # "acme"'s secret, HS256 over header.payload), and tokens the Elixir SDK
  # signs from each vector's inputs, counted from now (the vectors' fixed
  # clock is past what verify allows), pass Token.verify with the claims
  # intact. Every SDK makes the vectors byte for byte, so this covers them
  # all.
  use ExUnit.Case, async: true

  alias PresenceRelay.Token

  Code.require_file(Path.expand("../../../sdk/elixir/lib/arugula_presence.ex", __DIR__))

  @vectors Path.expand("../../../sdk/vectors.json", __DIR__) |> File.read!() |> Jason.decode!()

  test "every vector is signed as the relay checks" do
    for %{"name" => name, "input" => %{"app" => app}, "token" => token} <- @vectors["valid"] do
      [h, p, sig] = String.split(token, ".")

      assert %{"alg" => "HS256", "typ" => "JWT"} =
               h |> Base.url_decode64!(padding: false) |> Jason.decode!(),
             name

      mac = :crypto.mac(:hmac, :sha256, PresenceRelay.Apps.secret(app), h <> "." <> p)
      assert Base.url_encode64(mac, padding: false) == sig, name
    end
  end

  test "the SDK's tokens from every vector's inputs pass verify" do
    for %{"name" => name, "input" => i} <- @vectors["valid"] do
      opts =
        [
          app: i["app"],
          secret: i["secret"],
          room: i["room"],
          user_id: i["user_id"],
          name: i["user_name"]
        ] ++
          if(i["look"], do: [look: i["look"]], else: []) ++
          if(i["ttl"], do: [ttl: i["ttl"]], else: [])

      assert {:ok, t} = Token.verify(ArugulaPresence.create_room_token(opts)), name

      assert {t.app, t.room, t.sub, t.name, t.look} ==
               {i["app"], i["room"], i["user_id"], i["user_name"], i["look"]},
             name
    end
  end
end
