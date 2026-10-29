defmodule ArugulaPresence do
  @moduledoc """
  Sign an arugula.stream room token on your server (PROTOCOL.md, "Room
  token"): an HS256 JWT with the claims in a fixed order and compact JSON,
  so every arugula SDK makes the same bytes for the same inputs
  (../vectors.json). `:crypto` and Jason.

      ArugulaPresence.create_room_token(
        app: "acme", secret: secret, room: "design-team",
        user_id: user.id, name: user.name
      )
  """

  @room ~r/\A[A-Za-z0-9_.:-]{1,128}\z/
  @sub ~r/\A[A-Za-z0-9_.:@-]{1,128}\z/
  # Control characters (and the two line separators some JSON encoders
  # escape) aren't names: refusing them keeps every SDK's JSON identical.
  @control ~r/[\x{0000}-\x{001f}\x{007f}\x{2028}\x{2029}]/u
  @max_ttl 86_400
  @header ~s({"alg":"HS256","typ":"JWT"})

  @doc """
  A room token for `:user_id` (called `:name`) in `:room`, signed with
  your app's `:secret`. Options: `:app`, `:secret`, `:room`, `:user_id`,
  `:name` (required); `:look` (`%{character: n, colour: n}`), `:ttl`
  (seconds, default 3600, at most 86400), `:now` (Unix seconds, tests).
  Raises `ArgumentError` on bad input.
  """
  @spec create_room_token(keyword) :: String.t()
  def create_room_token(opts) do
    app = Keyword.get(opts, :app)
    secret = Keyword.get(opts, :secret)
    room = Keyword.get(opts, :room)
    user_id = Keyword.get(opts, :user_id)
    name = Keyword.get(opts, :name)
    look = Keyword.get(opts, :look)
    ttl = Keyword.get(opts, :ttl, 3600)

    check(is_binary(app) and app =~ @room, "app: your app's id, 1–128 of A–Z a–z 0–9 _ . : -")
    check(is_binary(secret) and secret != "", "secret: your app's secret")
    check(is_binary(room) and room =~ @room, "room: 1–128 of A–Z a–z 0–9 _ . : -")
    check(is_binary(user_id) and user_id =~ @sub, "user_id: 1–128 of A–Z a–z 0–9 _ . : @ -")

    check(
      is_binary(name) and String.valid?(name) and name != "" and
        length(String.to_charlist(name)) <= 80 and not (name =~ @control),
      "name: 1–80 characters, no control characters"
    )

    check(is_integer(ttl) and ttl >= 1 and ttl <= @max_ttl, "ttl: 1–86400 seconds")
    look = look && normalise(look)
    check(look != :error, "look: %{character: n, colour: n}, each 0–255")

    now = Keyword.get_lazy(opts, :now, fn -> System.system_time(:second) end)

    claims =
      ~s({"app":#{str(app)},"room":#{str(room)},"sub":#{str(user_id)},"name":#{str(name)},"exp":#{now + ttl}) <>
        if(look,
          do: ~s(,"look":{"character":#{look.character},"colour":#{look.colour}}),
          else: ""
        ) <>
        "}"

    body = b64(@header) <> "." <> b64(claims)
    body <> "." <> b64(:crypto.mac(:hmac, :sha256, secret, body))
  end

  defp normalise(look) when is_map(look) do
    ch = Map.get(look, :character, Map.get(look, "character"))
    co = Map.get(look, :colour, Map.get(look, "colour"))

    if is_integer(ch) and ch in 0..255 and is_integer(co) and co in 0..255,
      do: %{character: ch, colour: co},
      else: :error
  end

  defp normalise(_), do: :error

  defp check(true, _), do: :ok
  defp check(_, why), do: raise(ArgumentError, why)

  defp str(s), do: Jason.encode!(s)

  defp b64(data), do: Base.url_encode64(data, padding: false)
end
