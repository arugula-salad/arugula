defmodule PresenceRelayWeb.DemoController do
  @moduledoc """
  `POST /api/demo/token` (PROTOCOL.md, "Room token"): on a demo relay,
  a token for app `demo` to anyone, for any room, by name; `sub` is random.
  So many an hour per client IP (`PresenceRelay.Limits`' `demo_token`).
  """
  use Phoenix.Controller, formats: [:json]

  alias PresenceRelay.Token

  plug PresenceRelayWeb.RateLimit, :demo_token

  @life 12 * 3600

  def token(conn, params) do
    room = params["room"]
    name = params["name"]

    cond do
      not PresenceRelay.Apps.demo?() ->
        conn |> put_status(404) |> json(%{error: "no demo here"})

      not Token.valid_room?(room) ->
        conn |> put_status(400) |> json(%{error: "a room is 1–128 of A–Z a–z 0–9 _ . : -"})

      not Token.valid_name?(name) ->
        conn |> put_status(400) |> json(%{error: "a name is 1–80 characters"})

      true ->
        sub = "u_" <> Base.encode16(:crypto.strong_rand_bytes(6), case: :lower)
        exp = System.system_time(:second) + @life
        claims = %{"room" => room, "sub" => sub, "name" => name, "exp" => exp}

        claims =
          if look = Token.look(params["look"]), do: Map.put(claims, "look", look), else: claims

        json(conn, %{token: Token.sign("demo", claims), room: room, sub: sub})
    end
  end
end
