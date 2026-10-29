defmodule PresenceRelay.Billing.Stripe do
  @moduledoc """
  The little of Stripe's API billing uses (#318's P11), on Finch (already
  the relay's HTTP client, for mail and sign-in keys): no SDK, since
  Elixir has no official one and we need half a dozen calls.

  Requests are form-encoded as Stripe expects (`a[b][0][c]=v`), with the
  secret key as basic auth. `api_base` (config `:stripe`'s, default
  `https://api.stripe.com`) points tests at stripe-mock or a fake.

  Also: checking a webhook's `Stripe-Signature`.
  """

  @timeout 15_000
  @tolerance 300

  @doc """
  `method` `path` (`"/v1/…"`) with `params`, as `{:ok, map}` for a 2xx, or
  `{:error, {status, map}}` / `{:error, reason}`. `opts`: `:idempotency_key`.
  """
  def request(config, method, path, params \\ %{}, opts \\ []) do
    {url, body} =
      case method do
        :get ->
          q = encode(params)
          {config.api_base <> path <> if(q == "", do: "", else: "?" <> q), nil}

        _ ->
          {config.api_base <> path, encode(params)}
      end

    headers =
      [
        {"authorization", "Bearer " <> config.secret_key},
        {"content-type", "application/x-www-form-urlencoded"},
        {"accept", "application/json"}
      ] ++
        case opts[:idempotency_key] do
          nil -> []
          key -> [{"idempotency-key", key}]
        end ++
        case config[:api_version] do
          nil -> []
          v -> [{"stripe-version", v}]
        end

    req = Finch.build(method, url, headers, body)

    case Finch.request(req, PresenceRelay.Finch, receive_timeout: @timeout) do
      {:ok, %Finch.Response{status: s, body: b}} when s in 200..299 ->
        case Jason.decode(b) do
          {:ok, map} -> {:ok, map}
          _ -> {:error, {s, %{"error" => %{"message" => "not JSON"}}}}
        end

      {:ok, %Finch.Response{status: s, body: b}} ->
        {:error, {s, decode_error(b)}}

      {:error, e} ->
        {:error, e}
    end
  end

  defp decode_error(b) do
    case Jason.decode(b) do
      {:ok, %{} = map} -> map
      _ -> %{"error" => %{"message" => String.slice(b, 0, 200)}}
    end
  end

  @doc "A short reason for a failed request, for logs and the dashboard (never the key)."
  def describe({status, %{"error" => %{"message" => m}}}) when is_integer(status),
    do: "Stripe said #{status}: #{m}"

  def describe({status, _}) when is_integer(status), do: "Stripe said #{status}"
  def describe(%{__exception__: true} = e), do: "couldn't reach Stripe: #{Exception.message(e)}"
  def describe(other), do: "couldn't reach Stripe: #{inspect(other)}"

  @doc """
  Stripe's form encoding: nested maps as `a[b]`, lists as `a[0]`, keys in
  order (sorted) so the same params make the same body.
  """
  def encode(params) do
    params
    |> pairs(nil)
    |> Enum.map_join("&", fn {k, v} -> URI.encode_www_form(k) <> "=" <> URI.encode_www_form(v) end)
  end

  defp pairs(map, prefix) when is_map(map) do
    map
    |> Enum.reject(fn {_, v} -> is_nil(v) end)
    |> Enum.sort_by(fn {k, _} -> to_string(k) end)
    |> Enum.flat_map(fn {k, v} -> pairs(v, key(prefix, k)) end)
  end

  defp pairs(list, prefix) when is_list(list) do
    list
    |> Enum.with_index()
    |> Enum.flat_map(fn {v, i} -> pairs(v, key(prefix, i)) end)
  end

  defp pairs(v, prefix), do: [{prefix, to_string(v)}]

  defp key(nil, k), do: to_string(k)
  defp key(prefix, k), do: "#{prefix}[#{k}]"

  @doc """
  Whether `payload` (the raw body) is signed by `secret` per `header` (its
  `Stripe-Signature`: `t=<unix>,v1=<hex hmac>[,v1=…]`), and recent: the
  timestamp within `tolerance` seconds (5 minutes) of `now`, either way.
  `:ok`, or `{:error, why}`.
  """
  def verify(payload, header, secret, now \\ System.system_time(:second), tolerance \\ @tolerance)

  def verify(payload, header, secret, now, tolerance)
      when is_binary(payload) and is_binary(header) and is_binary(secret) do
    parts =
      header
      |> String.split(",")
      |> Enum.map(&String.split(String.trim(&1), "=", parts: 2))

    t =
      Enum.find_value(parts, fn
        [k, v] when k == "t" -> v
        _ -> nil
      end)

    sigs = for [k, v] <- parts, k == "v1", do: v

    with {:ok, t} <- parse_t(t),
         :ok <- if(sigs == [], do: {:error, "no v1 signature"}, else: :ok),
         :ok <- if(abs(now - t) <= tolerance, do: :ok, else: {:error, "too old (or too new)"}) do
      expected =
        :crypto.mac(:hmac, :sha256, secret, "#{t}.#{payload}") |> Base.encode16(case: :lower)

      if Enum.any?(sigs, &Plug.Crypto.secure_compare(&1, expected)),
        do: :ok,
        else: {:error, "bad signature"}
    end
  end

  def verify(_, _, _, _, _), do: {:error, "no signature"}

  defp parse_t(nil), do: {:error, "no timestamp"}

  defp parse_t(t) do
    case Integer.parse(t) do
      {n, ""} -> {:ok, n}
      _ -> {:error, "bad timestamp"}
    end
  end

  @doc "A `Stripe-Signature` header for `payload` (tests, and the fake Stripe)."
  def sign(payload, secret, t \\ System.system_time(:second)) do
    sig = :crypto.mac(:hmac, :sha256, secret, "#{t}.#{payload}") |> Base.encode16(case: :lower)
    "t=#{t},v1=#{sig}"
  end
end
