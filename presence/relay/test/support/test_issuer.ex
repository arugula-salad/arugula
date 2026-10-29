defmodule PresenceRelay.TestIssuer do
  @moduledoc """
  A sign-in provider of the tests' own (#318 P4): RSA, EC and Ed25519
  keys made here, a JWKS and an OIDC discovery document served by Bandit
  on 127.0.0.1, and tokens signed with those keys. Counts each path's
  requests, so a test can see when the relay fetched.
  """

  defmodule Plug do
    @moduledoc false
    @behaviour Elixir.Plug
    import Elixir.Plug.Conn

    @impl true
    def init(agent), do: agent

    @impl true
    def call(conn, agent) do
      s =
        Agent.get_and_update(agent, fn s ->
          s = update_in(s.hits, &Map.update(&1, conn.request_path, 1, fn n -> n + 1 end))
          {s, s}
        end)

      if s[:delay], do: Process.sleep(s.delay)

      cond do
        conn.request_path == "/.well-known/openid-configuration" ->
          send_json(conn, s, %{issuer: s.issuer, jwks_uri: s.url <> "/keys/jwks.json"})

        String.ends_with?(conn.request_path, "jwks.json") ->
          send_json(conn, s, %{keys: Enum.map(s.keys, & &1.jwk)})

        true ->
          send_resp(conn, 404, "")
      end
    end

    defp send_json(conn, s, body) do
      conn
      |> put_resp_content_type("application/json")
      |> put_resp_header("cache-control", s.cache)
      |> send_resp(200, Jason.encode!(body))
    end
  end

  @doc """
  Starts an issuer serving `keys` (under the test's supervisor): `%{url,
  agent}`. Its discovery document names `issuer` (default: its own URL).
  """
  def start(keys, opts \\ []) do
    {:ok, agent} = Agent.start_link(fn -> %{keys: keys, hits: %{}, cache: "max-age=600"} end)

    pid =
      ExUnit.Callbacks.start_supervised!(
        {Bandit, plug: {Plug, agent}, ip: :loopback, port: 0, startup_log: false},
        id: make_ref()
      )

    {:ok, {_, port}} = ThousandIsland.listener_info(pid)
    url = "http://127.0.0.1:#{port}"
    Agent.update(agent, &Map.merge(&1, %{url: url, issuer: opts[:issuer] || url}))
    %{url: url, agent: agent}
  end

  def set_keys(%{agent: a}, keys), do: Agent.update(a, &%{&1 | keys: keys})
  def set(%{agent: a}, k, v), do: Agent.update(a, &Map.put(&1, k, v))
  def hits(%{agent: a}, path), do: Agent.get(a, &Map.get(&1.hits, path, 0))

  def jwks_hits(%{agent: a}),
    do:
      Agent.get(a, fn s ->
        s.hits
        |> Enum.filter(fn {p, _} -> String.ends_with?(p, "jwks.json") end)
        |> Enum.map(&elem(&1, 1))
        |> Enum.sum()
      end)

  ## Keys

  def rsa(kid, bits \\ 2048) do
    {[e, n], priv} = :crypto.generate_key(:rsa, {bits, 65537})

    %{
      kid: kid,
      alg: "RS256",
      priv: priv,
      jwk: %{kty: "RSA", kid: kid, use: "sig", alg: "RS256", n: b64(n), e: b64(e)}
    }
  end

  def ec(kid) do
    {<<4, x::binary-size(32), y::binary-size(32)>>, priv} =
      :crypto.generate_key(:ecdh, :secp256r1)

    %{
      kid: kid,
      alg: "ES256",
      priv: priv,
      jwk: %{kty: "EC", kid: kid, crv: "P-256", x: b64(x), y: b64(y)}
    }
  end

  def ed(kid) do
    {pub, priv} = :crypto.generate_key(:eddsa, :ed25519)

    %{
      kid: kid,
      alg: "EdDSA",
      priv: priv,
      jwk: %{kty: "OKP", kid: kid, crv: "Ed25519", x: b64(pub)}
    }
  end

  ## Tokens

  @doc "A token with `claims` (an hour's `exp` unless given), signed with `key`; `header` overrides."
  def sign(key, claims, header \\ %{}) do
    now = System.system_time(:second)
    claims = Map.merge(%{"exp" => now + 3600, "iat" => now}, claims)
    h = Map.merge(%{"alg" => key.alg, "typ" => "JWT", "kid" => key.kid}, header)
    input = enc(h) <> "." <> enc(claims)
    input <> "." <> b64(signature(key, h["alg"], input))
  end

  defp signature(%{priv: priv}, "RS256", input), do: :crypto.sign(:rsa, :sha256, input, priv)
  defp signature(%{priv: priv}, "RS512", input), do: :crypto.sign(:rsa, :sha512, input, priv)

  defp signature(%{priv: priv}, "EdDSA", input),
    do: :crypto.sign(:eddsa, :none, input, [priv, :ed25519])

  defp signature(_, "HS256", input), do: :crypto.mac(:hmac, :sha256, "guess", input)
  defp signature(_, "none", _), do: ""

  defp signature(%{priv: priv}, "ES256", input) do
    # :crypto signs DER; a JWS carries r || s, 32 bytes each.
    <<0x30, _, 0x02, rl, r::binary-size(rl), 0x02, sl, s::binary-size(sl)>> =
      :crypto.sign(:ecdsa, :sha256, input, [priv, :secp256r1])

    pad(r) <> pad(s)
  end

  defp pad(i) do
    i = :binary.decode_unsigned(i)
    <<i::unsigned-big-size(256)>>
  end

  defp enc(map), do: b64(Jason.encode!(map))
  defp b64(bin), do: Base.url_encode64(bin, padding: false)
end
