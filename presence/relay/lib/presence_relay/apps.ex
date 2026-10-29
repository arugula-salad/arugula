defmodule PresenceRelay.Apps do
  @moduledoc """
  The host apps this relay serves.

  - **Static apps** from `PRESENCE_APPS` (`id:secret,…`), and on a demo
    relay (`DEMO=1`) app `"demo"`, its secret made at boot (or
    `DEMO_SECRET`), for which `POST /api/demo/token` hands out tokens to
    anyone.
  - **Apps in the database** (#318 P3): an owner, an HS256 secret for
    server-signed room tokens (encrypted at rest with a key derived from
    `SECRET_KEY_BASE`: HS256 is symmetric, so it has to come back),
    publishable keys, allowed origins, and whether guests may join.
    `meet` (meet.arugula.stream) is one, owned by the system account.

  `secret/1` serves both, the database's through a short cache (a rotated
  secret takes up to `@cache_ms` to reach every node).
  """

  import Ecto.Query
  alias PresenceRelay.Repo
  alias PresenceRelay.Accounts.Account
  alias PresenceRelay.Apps.{App, Key, Origin}

  @key {__MODULE__, :apps}
  @cache __MODULE__.Cache
  @cache_ms 60_000
  @salt "presence app secret"
  @system "acc_system"

  ## Static apps

  @doc "Reads the static apps from config (adding the demo app if the demo is on)."
  def load do
    apps = Application.get_env(:presence_relay, :apps, %{})

    apps =
      if demo?(),
        do:
          Map.put(
            apps,
            "demo",
            Application.get_env(:presence_relay, :demo_secret) ||
              Base.url_encode64(:crypto.strong_rand_bytes(32), padding: false)
          ),
        else: apps

    :persistent_term.put(@key, apps)

    if :ets.whereis(@cache) == :undefined,
      do: :ets.new(@cache, [:named_table, :public, :set, read_concurrency: true])

    :ok
  end

  @doc "The app's HS256 secret, or nil for an app this relay doesn't know."
  def secret(app) when is_binary(app) do
    case Map.fetch(:persistent_term.get(@key, %{}), app) do
      {:ok, secret} -> secret
      :error -> db_secret(app)
    end
  end

  def secret(_), do: nil

  def demo?, do: Application.get_env(:presence_relay, :demo, false) == true

  @doc "Whether there's a database (accounts, apps)."
  def database?, do: Application.get_env(:presence_relay, :database, true) == true

  # Only ids the database could hold are looked up (a token naming any
  # other app costs no query), and only secrets found are cached, so
  # made-up ids can't grow the cache.
  defp db_secret(app) do
    now = System.monotonic_time(:millisecond)
    db_id? = app == "meet" or Regex.match?(~r/\Aapp_[0-9a-f]{16}\z/, app)

    case db_id? && database?() && :ets.whereis(@cache) != :undefined && :ets.lookup(@cache, app) do
      [{^app, secret, at}] when now - at < @cache_ms ->
        secret

      false ->
        nil

      _ ->
        secret = with %App{secret_enc: enc} <- Repo.get(App, app), do: decrypt(enc)
        if secret, do: :ets.insert(@cache, {app, secret, now})
        secret
    end
  end

  defp forget(app), do: if(:ets.whereis(@cache) != :undefined, do: :ets.delete(@cache, app))

  defp encrypt(secret), do: Plug.Crypto.encrypt(key_base(), @salt, secret)

  defp decrypt(enc) do
    case Plug.Crypto.decrypt(key_base(), @salt, enc, max_age: :infinity) do
      {:ok, secret} -> secret
      _ -> nil
    end
  end

  defp key_base, do: PresenceRelayWeb.Endpoint.config(:secret_key_base)

  ## Apps in the database

  @doc "The apps `owner` owns, with keys and origins."
  def list(%Account{id: owner}) do
    Repo.all(
      from(a in App,
        where: a.owner_id == ^owner,
        order_by: a.inserted_at,
        preload: [:keys, :origins]
      )
    )
  end

  @doc "`owner`'s app `id`, with keys and origins, or nil."
  def get(%Account{id: owner}, id) do
    Repo.one(
      from(a in App, where: a.id == ^id and a.owner_id == ^owner, preload: [:keys, :origins])
    )
  end

  @doc """
  A new app for `owner`: `{:ok, app, secret}`, with a live publishable key.
  The secret is shown this once; the dashboard can rotate it.
  """
  def create(%Account{} = owner, attrs) do
    name = attrs["name"] || attrs[:name]
    guests = (attrs["guests"] || attrs[:guests]) == true

    if valid_name?(name) do
      secret = new_secret()
      id = attrs[:id] || "app_" <> rand_hex(8)

      Repo.transaction(fn ->
        app =
          Repo.insert!(%App{
            id: id,
            owner_id: owner.id,
            name: name,
            guests: guests,
            secret_enc: encrypt(secret)
          })

        Repo.insert!(%Key{app_id: app.id, key: new_key("live"), mode: "live"})
        {Repo.preload(app, [:keys, :origins]), secret}
      end)
      |> case do
        {:ok, {app, secret}} -> {:ok, app, secret}
        error -> error
      end
    else
      {:error, "a name is 1–80 characters"}
    end
  end

  @doc "A name and whether guests may join."
  def update(%App{} = app, attrs) do
    changes =
      %{}
      |> then(fn c ->
        if valid_name?(attrs["name"]), do: Map.put(c, :name, attrs["name"]), else: c
      end)
      |> then(fn c ->
        if is_boolean(attrs["guests"]), do: Map.put(c, :guests, attrs["guests"]), else: c
      end)

    app |> Ecto.Changeset.change(changes) |> Repo.update()
  end

  @doc "A new secret for `app`: `{:ok, secret}`, shown once; the old one stops working."
  def rotate_secret(%App{} = app) do
    secret = new_secret()

    with {:ok, _} <- app |> Ecto.Changeset.change(secret_enc: encrypt(secret)) |> Repo.update() do
      forget(app.id)
      {:ok, secret}
    end
  end

  @doc "Allows `origin` (normalized) for `app`'s publishable keys."
  def add_origin(%App{} = app, origin) do
    case normalize_origin(origin) do
      {:ok, o} ->
        Repo.insert(%Origin{app_id: app.id, origin: o},
          on_conflict: :nothing,
          conflict_target: [:app_id, :origin]
        )

        {:ok, o}

      error ->
        error
    end
  end

  def remove_origin(%App{} = app, origin) do
    case normalize_origin(origin) do
      {:ok, o} ->
        Repo.delete_all(from(x in Origin, where: x.app_id == ^app.id and x.origin == ^o))
        {:ok, o}

      error ->
        error
    end
  end

  @doc "A new publishable key for `app` (`\"live\"` or `\"test\"`)."
  def make_key(%App{} = app, mode) when mode in ~w(live test) do
    Repo.insert(%Key{app_id: app.id, key: new_key(mode), mode: mode})
  end

  def make_key(_, _), do: {:error, "a key is live or test"}

  def revoke_key(%App{} = app, key) do
    now = DateTime.utc_now() |> DateTime.truncate(:second)

    case Repo.update_all(
           from(k in Key, where: k.app_id == ^app.id and k.key == ^key and is_nil(k.revoked_at)),
           set: [revoked_at: now]
         ) do
      {1, _} -> :ok
      _ -> {:error, :not_found}
    end
  end

  @doc """
  The app a publishable key opens, with its origins and issuer, and the
  key's mode; nil for a key that's unknown or revoked (or no database).
  """
  def by_key(key) when is_binary(key) do
    if database?() and key =~ ~r/\Apk_(live|test)_[A-Za-z0-9_-]{10,64}\z/ do
      Repo.one(
        from(k in Key,
          join: a in assoc(k, :app),
          where: k.key == ^key and is_nil(k.revoked_at),
          preload: [app: {a, [:origins, :issuer]}]
        )
      )
      |> case do
        nil -> nil
        %Key{app: app, mode: mode} -> %{app: app, mode: mode}
      end
    end
  end

  def by_key(_), do: nil

  @doc """
  Whether a page at `origin` (its Origin header) may use a key in `mode`
  for an app allowing `allowed`: one of them (`*.` matches subdomains),
  and for a test key any localhost.
  """
  def origin_allowed?(allowed, mode, origin) when is_binary(origin) do
    o = URI.parse(origin)

    cond do
      o.scheme not in ["http", "https"] or is_nil(o.host) -> false
      mode == "test" and o.host in ["localhost", "127.0.0.1", "[::1]", "::1"] -> true
      true -> Enum.any?(allowed, &origin_match?(&1, o))
    end
  end

  def origin_allowed?(_, _, _), do: false

  defp origin_match?(allowed, o) do
    a = URI.parse(allowed)

    a.scheme == o.scheme and a.port == o.port and
      case a.host do
        "*." <> rest -> String.ends_with?(o.host, "." <> rest)
        host -> host == o.host
      end
  end

  @doc "An origin as kept: `scheme://host[:port]`, lowercased, no path."
  def normalize_origin(origin) when is_binary(origin) do
    u = URI.parse(String.trim(origin))
    host = u.host && String.downcase(u.host)

    cond do
      u.scheme not in ["http", "https"] ->
        {:error, "an origin is http(s)://host[:port]"}

      is_nil(host) or host == "" or not (host =~ ~r/\A(\*\.)?[a-z0-9.\-\[\]:]+\z/) ->
        {:error, "an origin needs a host"}

      u.path not in [nil, "", "/"] or u.query != nil ->
        {:error, "an origin has no path"}

      true ->
        default = if u.scheme == "https", do: 443, else: 80
        port = if u.port && u.port != default, do: ":#{u.port}", else: ""
        {:ok, "#{u.scheme}://#{host}#{port}"}
    end
  end

  def normalize_origin(_), do: {:error, "an origin is http(s)://host[:port]"}

  @doc """
  The system account and app `meet` (meet.arugula.stream): guests allowed,
  its origins from `MEET_ORIGINS` (config `:meet_origins`). Idempotent;
  run at boot where there's a database.
  """
  def ensure_meet! do
    Repo.insert!(%Account{id: @system, kind: "system", name: "arugula.stream"},
      on_conflict: :nothing
    )

    unless Repo.get(App, "meet") do
      {:ok, _, _} =
        create(Repo.get!(Account, @system), %{
          id: "meet",
          name: "meet.arugula.stream",
          guests: true
        })
    end

    app = Repo.get!(App, "meet")

    for o <- Application.get_env(:presence_relay, :meet_origins, ["https://meet.arugula.stream"]),
        do: add_origin(app, o)

    :ok
  end

  defp valid_name?(n), do: is_binary(n) and String.trim(n) != "" and String.length(n) <= 80
  defp new_secret, do: Base.url_encode64(:crypto.strong_rand_bytes(32), padding: false)

  defp new_key(mode),
    do: "pk_#{mode}_" <> Base.url_encode64(:crypto.strong_rand_bytes(18), padding: false)

  defp rand_hex(n), do: Base.encode16(:crypto.strong_rand_bytes(n), case: :lower)
end
