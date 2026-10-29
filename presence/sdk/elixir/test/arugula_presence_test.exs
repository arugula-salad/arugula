defmodule ArugulaPresenceTest do
  # Every vector in ../../vectors.json, byte for byte, and every invalid
  # input refused.
  use ExUnit.Case, async: true

  @vectors Path.expand("../../vectors.json", __DIR__) |> File.read!() |> Jason.decode!()

  defp opts(i) do
    [
      app: i["app"],
      secret: i["secret"],
      room: i["room"],
      user_id: i["user_id"],
      name: i["user_name"],
      now: i["now"]
    ]
    |> then(&if(i["look"], do: &1 ++ [look: i["look"]], else: &1))
    |> then(&if(i["ttl"], do: &1 ++ [ttl: i["ttl"]], else: &1))
  end

  test "makes every vector" do
    for v <- @vectors["valid"],
        do: assert(ArugulaPresence.create_room_token(opts(v["input"])) == v["token"], v["name"])
  end

  test "refuses every invalid input" do
    for v <- @vectors["invalid"] do
      assert_raise ArgumentError, fn -> ArugulaPresence.create_room_token(opts(v["input"])) end
    end
  end

  test "ttl 0 is refused, not taken as the default" do
    assert_raise ArgumentError, fn ->
      ArugulaPresence.create_room_token(
        app: "a",
        secret: "s",
        room: "r",
        user_id: "u",
        name: "A",
        ttl: 0
      )
    end
  end

  test "now defaults to the clock" do
    [_, p, _] =
      String.split(
        ArugulaPresence.create_room_token(
          app: "a",
          secret: "s",
          room: "r",
          user_id: "u",
          name: "A"
        ),
        "."
      )

    %{"exp" => exp} = p |> Base.url_decode64!(padding: false) |> Jason.decode!()
    assert abs(exp - (System.system_time(:second) + 3600)) <= 2
  end
end
