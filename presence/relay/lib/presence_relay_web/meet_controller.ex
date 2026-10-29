defmodule PresenceRelayWeb.MeetController do
  @moduledoc """
  `/api/v1/meet/…` (#318 P6), meet.arugula.stream's JSON, by session
  cookie:

  - `GET /session`: who's signed in (or nobody), meet's publishable key
    for guests, and which ways to sign in work here.
  - `GET /rooms`: a signed-in person's rooms, made and recent.
  - `POST /rooms {name}`: a new room, for anyone (a guest's has no maker).
  - `GET /rooms/:slug`: a room's name, for its page.
  - `POST /rooms/:slug/token {name?, look?}`: a signed-in person's room
    token (and the room goes in their recent ones).
  """
  use Phoenix.Controller, formats: [:json]

  alias PresenceRelay.Meet
  alias PresenceRelay.Accounts.Google

  # meet keeps its rooms in Postgres: a relay without one has no meet.
  plug :database
  # Anyone may make a room, a guest too: so many an hour per client IP
  # (#318's P12).
  plug PresenceRelayWeb.RateLimit, :meet_room when action == :create

  def session(conn, _) do
    account =
      case conn.assigns.account do
        %{} = a -> %{id: a.id, email: a.email, name: a.name}
        nil -> nil
      end

    json(conn, %{
      account: account,
      key: Meet.publishable_key(),
      sign_in: %{email: PresenceRelay.Mailer.ready?(), google: Google.enabled?()}
    })
  end

  def rooms(conn, _) do
    me = conn.assigns.account
    %{made: made, recent: recent} = Meet.rooms(me)

    json(conn, %{
      made: Enum.map(made, &view(&1, me)),
      recent: Enum.map(recent, fn {room, at} -> Map.put(view(room, me), :joined_at, at) end)
    })
  end

  def create(conn, params) do
    me = conn.assigns.account

    case Meet.create(params["name"], me) do
      {:ok, room} -> conn |> put_status(201) |> json(%{room: view(room, me)})
      {:error, msg} -> conn |> put_status(400) |> json(%{error: msg})
    end
  end

  def show(conn, %{"slug" => slug}) do
    with_room(conn, slug, &json(conn, %{room: view(&1, conn.assigns.account)}))
  end

  def token(conn, %{"slug" => slug} = params) do
    with_room(conn, slug, fn room ->
      {:ok, token, sub} = Meet.token(conn.assigns.account, room, params["name"], params["look"])
      json(conn, %{token: token, sub: sub, room: room.slug})
    end)
  end

  defp database(conn, _) do
    if PresenceRelay.Apps.database?(),
      do: conn,
      else: conn |> put_status(503) |> json(%{error: "meet isn't set up here"}) |> halt()
  end

  defp with_room(conn, slug, fun) do
    case Meet.get(slug) do
      nil -> conn |> put_status(404) |> json(%{error: "there's no room at that link"})
      room -> fun.(room)
    end
  end

  defp view(room, me) do
    %{
      slug: room.slug,
      name: room.name,
      created_at: room.created_at,
      mine: not is_nil(me) and room.creator_id == me.id
    }
  end
end
