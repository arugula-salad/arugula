defmodule PresenceRelay.Token do
  @moduledoc """
  Room tokens (PROTOCOL.md, "Room token"): an HS256 JWT a host app's
  server signs with its secret, naming the app, the room, the user (`sub`)
  and what to call them, living at most a day.

  HS256 by hand with `:crypto` (HMAC-SHA256, a constant-time compare):
  it's all a room token needs, and it keeps JOSE (which needs
  public_key's headers to build) out of the relay.
  """

  @room ~r/\A[A-Za-z0-9_.:-]{1,128}\z/
  @sub ~r/\A[A-Za-z0-9_.:@-]{1,128}\z/
  @max_life 24 * 3600
  # A clock a little ahead of ours isn't a forged token.
  @skew 60

  defstruct [:app, :room, :sub, :name, :exp, look: nil]

  @type t :: %__MODULE__{}

  @doc "The token's claims, checked; or why not."
  def verify(token) when is_binary(token) do
    with [h, p, sig] <- String.split(token, "."),
         {:ok, %{"alg" => "HS256"}} <- decode(h),
         {:ok, %{"app" => app} = claims} <- decode(p),
         secret when is_binary(secret) <- PresenceRelay.Apps.secret(app) || {:error, :unknown_app},
         {:ok, sig} <- Base.url_decode64(sig, padding: false),
         true <- same?(mac(secret, h <> "." <> p), sig) || {:error, :bad_signature} do
      claims(claims)
    else
      {:error, reason} -> {:error, reason}
      _ -> {:error, :invalid}
    end
  end

  def verify(_), do: {:error, :invalid}

  @doc "Signs a token for `app` (the demo endpoint, and tests)."
  def sign(app, claims) when is_map(claims) do
    secret = PresenceRelay.Apps.secret(app) || raise ArgumentError, "unknown app #{app}"
    h = encode(%{"alg" => "HS256", "typ" => "JWT"})
    p = encode(Map.put(claims, "app", app))
    h <> "." <> p <> "." <> Base.url_encode64(mac(secret, h <> "." <> p), padding: false)
  end

  def valid_room?(room), do: is_binary(room) and room =~ @room
  def valid_sub?(sub), do: is_binary(sub) and sub =~ @sub
  def valid_name?(name), do: is_binary(name) and name != "" and String.length(name) <= 80

  defp decode(part) do
    with {:ok, json} <- Base.url_decode64(part, padding: false),
         {:ok, map} when is_map(map) <- Jason.decode(json) do
      {:ok, map}
    else
      _ -> {:error, :invalid}
    end
  end

  defp encode(map), do: Base.url_encode64(Jason.encode!(map), padding: false)

  defp mac(secret, data), do: :crypto.mac(:hmac, :sha256, secret, data)

  # Constant time (hash_equals wants two of one size).
  defp same?(a, b), do: byte_size(a) == byte_size(b) and :crypto.hash_equals(a, b)

  defp claims(c) do
    now = System.system_time(:second)

    cond do
      not is_integer(c["exp"]) ->
        {:error, :no_expiry}

      c["exp"] <= now ->
        {:error, :expired}

      c["exp"] > now + @max_life + @skew ->
        {:error, :lives_too_long}

      not valid_room?(c["room"]) ->
        {:error, :bad_room}

      not valid_sub?(c["sub"]) ->
        {:error, :bad_sub}

      not valid_name?(c["name"]) ->
        {:error, :bad_name}

      true ->
        {:ok,
         struct(__MODULE__,
           app: c["app"],
           room: c["room"],
           sub: c["sub"],
           name: c["name"],
           exp: c["exp"],
           look: look(c["look"])
         )}
    end
  end

  @doc "A look (`{character, colour}`, bytes), or nil."
  def look(%{"character" => ch, "colour" => co})
      when is_integer(ch) and ch in 0..255 and is_integer(co) and co in 0..255,
      do: %{"character" => ch, "colour" => co}

  def look(_), do: nil
end
