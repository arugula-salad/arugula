defmodule PresenceRelayWeb.MeController do
  @moduledoc "`GET /api/v1/me`: who's signed in."
  use Phoenix.Controller, formats: [:json]

  def show(%{assigns: %{account: %{} = a}} = conn, _),
    do: json(conn, %{account: %{id: a.id, email: a.email, name: a.name}})

  def show(conn, _), do: conn |> put_status(401) |> json(%{account: nil})
end
