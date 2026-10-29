defmodule PresenceRelayWeb.AppsController do
  @moduledoc """
  `/api/v1/apps` (#318 P3), for the signed-in owner: make and list apps,
  rotate the secret (shown once), allow origins, make and revoke
  publishable keys, and set the sign-in provider whose tokens admit
  members (P4, `PresenceRelay.Issuers`), and read usage and set the cap
  on monthly active users (P9, `PresenceRelay.Usage`). The dashboard
  (P10) is its first client.
  """
  use Phoenix.Controller, formats: [:json]

  alias PresenceRelay.{Apps, Issuers, Usage}

  def index(conn, _), do: json(conn, %{apps: Enum.map(Apps.list(conn.assigns.account), &view/1)})

  def create(conn, params) do
    case Apps.create(conn.assigns.account, params) do
      {:ok, app, secret} -> conn |> put_status(201) |> json(%{app: view(app), secret: secret})
      {:error, msg} when is_binary(msg) -> conn |> put_status(400) |> json(%{error: msg})
      {:error, _} -> conn |> put_status(500) |> json(%{error: "couldn't make the app"})
    end
  end

  def show(conn, %{"id" => id}), do: with_app(conn, id, &json(conn, %{app: view(&1)}))

  def update(conn, %{"id" => id} = params) do
    with_app(conn, id, fn app ->
      {:ok, app} = Apps.update(app, params)
      json(conn, %{app: view(Apps.get(conn.assigns.account, app.id))})
    end)
  end

  def rotate_secret(conn, %{"id" => id}) do
    with_app(conn, id, fn app ->
      {:ok, secret} = Apps.rotate_secret(app)
      json(conn, %{secret: secret})
    end)
  end

  def add_origin(conn, %{"id" => id} = params) do
    with_app(conn, id, fn app ->
      case Apps.add_origin(app, params["origin"]) do
        {:ok, _} -> json(conn, %{app: view(Apps.get(conn.assigns.account, id))})
        {:error, msg} -> conn |> put_status(400) |> json(%{error: msg})
      end
    end)
  end

  def remove_origin(conn, %{"id" => id} = params) do
    with_app(conn, id, fn app ->
      case Apps.remove_origin(app, params["origin"]) do
        {:ok, _} -> json(conn, %{app: view(Apps.get(conn.assigns.account, id))})
        {:error, msg} -> conn |> put_status(400) |> json(%{error: msg})
      end
    end)
  end

  def make_key(conn, %{"id" => id} = params) do
    with_app(conn, id, fn app ->
      case Apps.make_key(app, params["mode"] || "live") do
        {:ok, key} -> conn |> put_status(201) |> json(%{key: key_view(key)})
        {:error, msg} when is_binary(msg) -> conn |> put_status(400) |> json(%{error: msg})
      end
    end)
  end

  def revoke_key(conn, %{"id" => id, "key" => key}) do
    with_app(conn, id, fn app ->
      case Apps.revoke_key(app, key) do
        :ok -> json(conn, %{revoked: key})
        {:error, :not_found} -> conn |> put_status(404) |> json(%{error: "no such key"})
      end
    end)
  end

  @doc "The app's issuer (`null` for none)."
  def issuer(conn, %{"id" => id}),
    do: with_app(conn, id, &json(conn, %{issuer: Issuers.view(Issuers.get(&1))}))

  @doc "Sets the app's issuer, replacing any: `{provider, issuer?, project?, audience?, name_claim?, room_rule?}`."
  def put_issuer(conn, %{"id" => id} = params) do
    with_app(conn, id, fn app ->
      case Issuers.put(app, Map.delete(params, "id")) do
        {:ok, issuer} -> json(conn, %{issuer: Issuers.view(issuer)})
        {:error, msg} -> conn |> put_status(400) |> json(%{error: msg})
      end
    end)
  end

  def delete_issuer(conn, %{"id" => id}) do
    with_app(conn, id, fn app ->
      :ok = Issuers.delete(app)
      json(conn, %{issuer: nil})
    end)
  end

  @doc """
  The app's usage (P9, `PresenceRelay.Usage`): `?months=N` (default 3, at
  most 24) months and the last 31 days, oldest first, its cap and this
  month.
  """
  def usage(conn, %{"id" => id} = params) do
    with_app(conn, id, fn app ->
      case months(params["months"]) do
        {:ok, n} -> json(conn, Usage.report(app, n))
        :error -> conn |> put_status(400) |> json(%{error: "months is 1–24"})
      end
    end)
  end

  @doc "Sets the app's cap on monthly active users: `{active_users: n | null}`."
  def put_cap(conn, %{"id" => id} = params) do
    with_app(conn, id, fn app ->
      with {:ok, cap} <- Map.fetch(params, "active_users") |> ok_or_missing(),
           {:ok, app} <- Usage.put_cap(app, cap) do
        json(conn, %{cap: %{active_users: Usage.cap(app)}})
      else
        {:error, msg} when is_binary(msg) -> conn |> put_status(400) |> json(%{error: msg})
        {:error, _} -> conn |> put_status(500) |> json(%{error: "couldn't set the cap"})
      end
    end)
  end

  defp months(nil), do: {:ok, 3}

  defp months(s) when is_binary(s) do
    case Integer.parse(s) do
      {n, ""} when n in 1..24 -> {:ok, n}
      _ -> :error
    end
  end

  defp months(_), do: :error

  defp ok_or_missing({:ok, v}), do: {:ok, v}
  defp ok_or_missing(:error), do: {:error, "send active_users: a whole number, or null"}

  defp with_app(conn, id, fun) do
    case Apps.get(conn.assigns.account, id) do
      nil -> conn |> put_status(404) |> json(%{error: "no such app"})
      app -> fun.(app)
    end
  end

  # Never the secret: it's shown once, when made or rotated.
  defp view(app) do
    %{
      id: app.id,
      name: app.name,
      guests: app.guests,
      origins: Enum.map(app.origins, & &1.origin),
      keys: app.keys |> Enum.reject(& &1.revoked_at) |> Enum.map(&key_view/1)
    }
  end

  defp key_view(k), do: %{key: k.key, mode: k.mode, made: k.inserted_at}
end
