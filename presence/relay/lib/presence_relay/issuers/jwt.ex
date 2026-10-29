defmodule PresenceRelay.Issuers.JWT do
  @moduledoc """
  A sign-in provider's token (a JWS, compact): parsed, its key picked from
  the provider's JWKS, its signature checked with `:crypto`.

  Only asymmetric algorithms, each with the key type it needs: RS256 (an
  RSA key of 2048 bits or more), ES256 (P-256) and EdDSA (Ed25519). `none`
  and HS* are refused: a JWKS issuer's keys are public, so an HMAC "signed"
  with one proves nothing. Like `PresenceRelay.Token`, by hand, without
  JOSE: the three checks are a few lines each.
  """

  @algs ~w(RS256 ES256 EdDSA)

  @doc "The header, the claims, what was signed and the signature; or `{:error, :malformed}`."
  def parse(token) when is_binary(token) do
    with [h, p, s] <- String.split(token, "."),
         {:ok, header} <- json(h),
         {:ok, claims} <- json(p),
         {:ok, sig} <- Base.url_decode64(s, padding: false) do
      {:ok, header, claims, h <> "." <> p, sig}
    else
      _ -> {:error, :malformed}
    end
  end

  def parse(_), do: {:error, :malformed}

  @doc "Whether the header names an algorithm a JWKS issuer may use."
  def alg_ok?(%{"alg" => alg}), do: alg in @algs
  def alg_ok?(_), do: false

  @doc """
  The JWK in `jwks` that can check a token with this header: the one with
  its `kid` (and a key type fit for its `alg`), or, when it names none,
  the set's only fitting key. Nil if there's none.
  """
  def key(%{"keys" => keys}, %{"alg" => alg} = header) when is_list(keys) do
    fitting = Enum.filter(keys, &(is_map(&1) and fits?(&1, alg)))

    case {header["kid"], fitting} do
      {kid, _} when is_binary(kid) -> Enum.find(fitting, &(&1["kid"] == kid))
      {nil, [only]} -> only
      _ -> nil
    end
  end

  def key(_, _), do: nil

  defp fits?(k, alg) do
    k["use"] in [nil, "sig"] and k["alg"] in [nil, alg] and
      case alg do
        "RS256" -> k["kty"] == "RSA"
        "ES256" -> k["kty"] == "EC" and k["crv"] == "P-256"
        "EdDSA" -> k["kty"] == "OKP" and k["crv"] == "Ed25519"
        _ -> false
      end
  end

  @doc "Whether `sig` is `jwk`'s signature of `input` under `alg`."
  def verify("RS256", %{"n" => n, "e" => e}, input, sig) do
    with {:ok, n} <- b64(n),
         {:ok, e} <- b64(e),
         true <- byte_size(strip(n)) >= 256 do
      safely(fn -> :crypto.verify(:rsa, :sha256, input, sig, [e, n]) end)
    else
      _ -> false
    end
  end

  def verify("ES256", %{"x" => x, "y" => y}, input, <<r::binary-size(32), s::binary-size(32)>>) do
    with {:ok, <<x::binary-size(32)>>} <- b64(x),
         {:ok, <<y::binary-size(32)>>} <- b64(y) do
      point = <<4, x::binary, y::binary>>
      safely(fn -> :crypto.verify(:ecdsa, :sha256, input, der(r, s), [point, :secp256r1]) end)
    else
      _ -> false
    end
  end

  def verify("EdDSA", %{"x" => x}, input, <<_::binary-size(64)>> = sig) do
    case b64(x) do
      {:ok, <<pub::binary-size(32)>>} ->
        safely(fn -> :crypto.verify(:eddsa, :none, input, sig, [pub, :ed25519]) end)

      _ ->
        false
    end
  end

  def verify(_, _, _, _), do: false

  # A JWS carries ES256 as r || s; :crypto wants an ASN.1 DER
  # Ecdsa-Sig-Value (a SEQUENCE of two INTEGERs), short enough for one
  # length byte.
  defp der(r, s) do
    body = int(r) <> int(s)
    <<0x30, byte_size(body), body::binary>>
  end

  defp int(b) do
    b = strip(b)
    b = if :binary.first(b) >= 0x80, do: <<0, b::binary>>, else: b
    <<0x02, byte_size(b), b::binary>>
  end

  # Leading zero bytes off, keeping one.
  defp strip(<<0, rest::binary>>) when rest != <<>>, do: strip(rest)
  defp strip(b), do: b

  # A malformed key (a point off the curve) raises in :crypto: not valid.
  defp safely(fun) do
    fun.() == true
  rescue
    _ -> false
  end

  defp b64(s) when is_binary(s), do: Base.url_decode64(s, padding: false)
  defp b64(_), do: :error

  defp json(part) do
    with {:ok, bin} <- Base.url_decode64(part, padding: false),
         {:ok, map} when is_map(map) <- Jason.decode(bin) do
      {:ok, map}
    else
      _ -> :error
    end
  end
end
