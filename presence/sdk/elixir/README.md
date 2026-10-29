# :arugula_presence (Elixir)

Sign arugula.stream room tokens on your server. `:crypto` and Jason (which
a Phoenix app already has).

Install (from this repo, until it's on Hex): in `mix.exs`,

    {:arugula_presence, path: "path/to/arugula/presence/sdk/elixir"}

```elixir
ArugulaPresence.create_room_token(
  app: "acme", secret: System.fetch_env!("ARUGULA_SECRET"), room: "design-team",
  user_id: user.id, name: user.name
)
```

Options: `:app`, `:secret`, `:room`, `:user_id`, `:name` (required);
`:look` (`%{character: n, colour: n}`), `:ttl` (seconds, default 3600, at
most 86400), `:now` (tests). Raises `ArgumentError` on bad input.

Phoenix controller:

```elixir
defmodule MyAppWeb.PresenceTokenController do
  use MyAppWeb, :controller

  def create(conn, %{"room" => room}) do
    user = conn.assigns.current_user
    # Your rule for who may be where.
    if room in user.team_rooms do
      token = ArugulaPresence.create_room_token(app: "acme", secret: System.fetch_env!("ARUGULA_SECRET"), room: room, user_id: to_string(user.id), name: user.name)
      json(conn, %{token: token})
    else
      send_resp(conn, 403, "")
    end
  end
end
```

Tests: `mix test` (every token in `../vectors.json`, byte for byte). The
toolchain is pinned in `mise.toml`, as for the relay.
