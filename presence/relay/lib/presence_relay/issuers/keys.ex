defmodule PresenceRelay.Issuers.Keys do
  @moduledoc """
  Sign-in providers' public documents (a JWKS, an OIDC discovery
  document), fetched and cached per URL, on each node.

  - Read from ETS by whoever asks (a socket's connect), no call while
    it's fresh: fresh as long as the response's `Cache-Control: max-age`
    says, held within 60 s and 24 h (an hour when it says nothing).
  - One fetch per URL at a time, in a task of this server's: whoever asks
    while it's out waits for it. A fetch has 5 s (config
    `:jwks_timeout_ms`), a body at most 256 KiB, and no redirects.
  - `refresh/1` (a token names a key the set lacks: the provider may have
    rotated) fetches again, but at most once per `refetch_ms` (config
    `:jwks_refetch_ms`, 30 s) per URL, so a stream of made-up key ids
    can't hammer the provider. A failed fetch waits as long before the
    next try, and the last good copy keeps working meanwhile.
  - Only `https://` to a public host: no IP literals, no localhost,
    `.local` or `.internal` names, and no name resolving to a private,
    loopback or link-local address (an app owner names the URL; it mustn't
    reach the relay's own network). Tests allow local hosts
    (`:issuer_allow_local`).
  """
  use GenServer

  @table __MODULE__
  @max_body 256 * 1024
  @min_ttl 60
  @max_ttl 24 * 3600
  @default_ttl 3600

  def start_link(_), do: GenServer.start_link(__MODULE__, nil, name: __MODULE__)

  @doc "The JSON document at `url`: `{:ok, map}` from the cache while fresh, else fetched; or `{:error, reason}`."
  def get(url) do
    case :ets.lookup(@table, url) do
      [{^url, doc, expires, _, _}] when is_map(doc) ->
        if now() < expires, do: {:ok, doc}, else: call({:fetch, url, :stale})

      _ ->
        call({:fetch, url, :stale})
    end
  end

  @doc "`url` fetched again, unless it was within `refetch_ms` (then the copy there is)."
  def refresh(url), do: call({:fetch, url, :refresh})

  @doc "Tests: forget every document."
  def clear, do: :ets.delete_all_objects(@table)

  defp call(msg) do
    GenServer.call(__MODULE__, msg, 2 * timeout() + 1_000)
  catch
    :exit, _ -> {:error, :unavailable}
  end

  @impl true
  def init(_) do
    :ets.new(@table, [:named_table, :public, :set, read_concurrency: true])
    {:ok, %{}}
  end

  # The state: url => {task ref, callers waiting}.
  @impl true
  def handle_call({:fetch, url, why}, from, out) do
    now = now()
    row = :ets.lookup(@table, url)
    window = refetch_ms()

    cond do
      Map.has_key?(out, url) ->
        {ref, waiting} = out[url]
        {:noreply, Map.put(out, url, {ref, [from | waiting]})}

      # Someone else's fetch made it fresh while this caller queued.
      why == :stale and match?([{_, doc, exp, _, _}] when is_map(doc) and now < exp, row) ->
        {:reply, {:ok, elem(hd(row), 1)}, out}

      match?([{_, _, _, tried, _}] when now - tried < window, row) ->
        [{_, doc, _, _, error}] = row
        {:reply, if(is_map(doc), do: {:ok, doc}, else: {:error, error || :rate_limited}), out}

      true ->
        %Task{ref: ref} = Task.async(fn -> fetch(url) end)
        {:noreply, Map.put(out, url, {ref, [from]})}
    end
  end

  @impl true
  def handle_info({ref, result}, out) when is_reference(ref) do
    Process.demonitor(ref, [:flush])
    {:noreply, done(out, ref, result)}
  end

  def handle_info({:DOWN, ref, :process, _, reason}, out),
    do: {:noreply, done(out, ref, {:error, {:crashed, reason}})}

  def handle_info(_, out), do: {:noreply, out}

  defp done(out, ref, result) do
    case Enum.find(out, fn {_, {r, _}} -> r == ref end) do
      nil ->
        out

      {url, {_, waiting}} ->
        now = now()

        reply =
          case result do
            {:ok, doc, ttl} ->
              :ets.insert(@table, {url, doc, now + ttl * 1000, now, nil})
              {:ok, doc}

            {:error, reason} ->
              # The last good copy (if any) stays, and keeps answering.
              {doc, exp} =
                case :ets.lookup(@table, url) do
                  [{_, doc, exp, _, _}] -> {doc, exp}
                  [] -> {nil, 0}
                end

              :ets.insert(@table, {url, doc, exp, now, reason})
              if is_map(doc), do: {:ok, doc}, else: {:error, reason}
          end

        for w <- waiting, do: GenServer.reply(w, reply)
        Map.delete(out, url)
    end
  end

  ## Fetching

  defp fetch(url) do
    with :ok <- fetchable(url),
         {:ok, {200, headers, body}} <- http_get(url),
         {:ok, doc} when is_map(doc) <- Jason.decode(body) do
      {:ok, doc, ttl(headers)}
    else
      {:ok, {status, _, _}} -> {:error, {:status, status}}
      {:ok, _} -> {:error, :not_json}
      {:error, reason} -> {:error, reason}
    end
  rescue
    e -> {:error, {:raised, Exception.message(e)}}
  end

  defp http_get(url) do
    req = Finch.build(:get, url, [{"accept", "application/json"}])

    step = fn
      {:status, s}, {_, h, b, n} ->
        {:cont, {s, h, b, n}}

      {:headers, hs}, {s, h, b, n} ->
        {:cont, {s, h ++ hs, b, n}}

      {:data, d}, {s, h, b, n} ->
        if n + byte_size(d) > @max_body,
          do: {:halt, :too_big},
          else: {:cont, {s, h, [b | d], n + byte_size(d)}}

      _, acc ->
        {:cont, acc}
    end

    case Finch.stream_while(req, PresenceRelay.Finch, {nil, [], [], 0}, step,
           receive_timeout: timeout(),
           request_timeout: timeout(),
           pool_timeout: timeout()
         ) do
      {:ok, :too_big} -> {:error, :too_big}
      {:ok, {status, headers, body, _}} -> {:ok, {status, headers, IO.iodata_to_binary(body)}}
      {:error, e, _} -> {:error, {:http, Exception.message(e)}}
    end
  end

  # max-age, held to the bounds; none (or no-store) is the default.
  defp ttl(headers) do
    cc = for({k, v} <- headers, String.downcase(k) == "cache-control", do: v) |> Enum.join(",")

    case Regex.run(~r/(?:^|[,\s])max-age\s*=\s*"?(\d+)/i, cc) do
      [_, n] -> n |> String.to_integer() |> max(@min_ttl) |> min(@max_ttl)
      _ -> @default_ttl
    end
  end

  @doc """
  Whether the relay may fetch `url`: `:ok`, or `{:error, reason}`. The
  checks in the moduledoc (a name resolving to a public address); a DNS
  answer that changes between this and the fetch isn't caught.
  """
  def fetchable(url) do
    u = URI.parse(url)

    cond do
      Application.get_env(:presence_relay, :issuer_allow_local, false) ->
        if u.scheme in ["http", "https"] and is_binary(u.host), do: :ok, else: {:error, :bad_url}

      u.scheme != "https" or not is_binary(u.host) ->
        {:error, :not_https}

      not public_name?(String.downcase(u.host)) ->
        {:error, :private_host}

      true ->
        host = to_charlist(u.host)

        addrs =
          Enum.flat_map([:inet, :inet6], fn family ->
            case :inet.getaddrs(host, family) do
              {:ok, list} -> list
              _ -> []
            end
          end)

        cond do
          addrs == [] -> {:error, :no_address}
          Enum.any?(addrs, &private?/1) -> {:error, :private_host}
          true -> :ok
        end
    end
  end

  @doc "Whether `host` is a DNS name that could be public (not an IP literal or a local name)."
  def public_name?(host) do
    String.contains?(host, ".") and match?({:error, _}, :inet.parse_address(to_charlist(host))) and
      not String.starts_with?(host, "[") and
      not Enum.any?(~w(.localhost .local .internal .lan .home.arpa), &String.ends_with?(host, &1)) and
      host =~ ~r/\A[a-z0-9.-]+\z/
  end

  defp private?({a, b, _, _}) do
    a in [0, 10, 127] or (a == 100 and b in 64..127) or (a == 169 and b == 254) or
      (a == 172 and b in 16..31) or (a == 192 and b == 168) or a >= 224
  end

  defp private?({0, 0, 0, 0, 0, 0xFFFF, hi, lo}),
    do: private?({div(hi, 256), rem(hi, 256), div(lo, 256), rem(lo, 256)})

  defp private?({a, _, _, _, _, _, _, _} = ip) do
    ip in [{0, 0, 0, 0, 0, 0, 0, 0}, {0, 0, 0, 0, 0, 0, 0, 1}] or
      Bitwise.band(a, 0xFE00) == 0xFC00 or Bitwise.band(a, 0xFFC0) == 0xFE80 or
      Bitwise.band(a, 0xFF00) == 0xFF00
  end

  defp timeout, do: Application.get_env(:presence_relay, :jwks_timeout_ms, 5_000)
  defp refetch_ms, do: Application.get_env(:presence_relay, :jwks_refetch_ms, 30_000)
  defp now, do: System.monotonic_time(:millisecond)
end
