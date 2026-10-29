defmodule PresenceRelay.Meet do
  @moduledoc """
  meet.arugula.stream's own data (#318 P6): its rooms and the rooms each
  account has been in lately. meet is an app on the relay like any other
  (app `meet`, PresenceRelay.Apps): its rooms are rooms on the socket,
  by slug.

  Into a room, a signed-in person gets a room token this server signs
  with meet's secret (`token/3`, which also notes the room as recent); a
  guest goes in by meet's publishable key and a name (`publishable_key/0`),
  the relay's guest path.
  """

  import Ecto.Query
  alias PresenceRelay.{Repo, Token}
  alias PresenceRelay.Accounts.Account
  alias PresenceRelay.Apps.Key
  alias PresenceRelay.Meet.{Recent, Room}

  @app "meet"
  # No i, l, o or u: none to mistake for 1 or 0 (or for each other) when
  # a slug is read aloud or retyped.
  @alphabet ~c"0123456789abcdefghjkmnpqrstvwxyz"
  @slug ~r/\A[0-9a-hjkmnp-tv-z]{4}-[0-9a-hjkmnp-tv-z]{4}\z/
  @token_life 12 * 3600
  @made 50
  @recent 20

  def app, do: @app

  def valid_slug?(slug), do: is_binary(slug) and slug =~ @slug

  @doc """
  A new room called `name`, made by `account` (nil for a guest):
  `{:ok, room}`, or `{:error, message}` for a bad name.
  """
  def create(name, account) do
    name = if is_binary(name), do: String.trim(name)

    if is_binary(name) and name != "" and String.length(name) <= 80 do
      insert(name, account && account.id, 3)
    else
      {:error, "a room's name is 1–80 characters"}
    end
  end

  # 40 bits a slug: a clash is rare, and tried again.
  defp insert(_name, _creator, 0), do: {:error, "couldn't make the room; try again"}

  defp insert(name, creator, tries) do
    %Room{slug: new_slug(), name: name, creator_id: creator}
    |> Ecto.Changeset.change()
    |> Ecto.Changeset.unique_constraint(:slug, name: :meet_rooms_pkey)
    |> Repo.insert()
    |> case do
      {:ok, room} -> {:ok, room}
      {:error, _} -> insert(name, creator, tries - 1)
    end
  end

  defp new_slug do
    s =
      for <<b <- :crypto.strong_rand_bytes(8)>>, into: "", do: <<Enum.at(@alphabet, rem(b, 32))>>

    String.slice(s, 0, 4) <> "-" <> String.slice(s, 4, 4)
  end

  @doc "The room with `slug`, or nil."
  def get(slug) do
    if valid_slug?(slug), do: Repo.get(Room, slug)
  end

  @doc """
  `account`'s rooms: the ones it made (newest first) and the others it's
  been in lately (last joined first).
  """
  def rooms(%Account{id: id}) do
    made =
      Repo.all(
        from(r in Room, where: r.creator_id == ^id, order_by: [desc: r.created_at], limit: @made)
      )

    recent =
      Repo.all(
        from(x in Recent,
          join: r in Room,
          on: r.slug == x.slug,
          where: x.account_id == ^id and (is_nil(r.creator_id) or r.creator_id != ^id),
          order_by: [desc: x.joined_at],
          limit: @recent,
          select: {r, x.joined_at}
        )
      )

    %{made: made, recent: recent}
  end

  @doc """
  A room token for `account` in `room`, as `name` (or its own name, or
  its address's first part) and in `look` if it's one, signed with
  meet's secret; notes the room as one `account` has been in.
  `{:ok, token, sub}`.
  """
  def token(%Account{} = account, %Room{slug: slug}, name, look \\ nil) do
    name = display_name(name, account)
    now = DateTime.utc_now() |> DateTime.truncate(:second)

    Repo.insert!(%Recent{account_id: account.id, slug: slug, joined_at: now},
      on_conflict: [set: [joined_at: now]],
      conflict_target: [:account_id, :slug]
    )

    claims = %{
      "room" => slug,
      "sub" => account.id,
      "name" => name,
      "exp" => System.system_time(:second) + @token_life
    }

    claims = if l = Token.look(look), do: Map.put(claims, "look", l), else: claims
    {:ok, Token.sign(@app, claims), account.id}
  end

  defp display_name(name, account) do
    name = if is_binary(name), do: String.trim(name), else: ""

    cond do
      Token.valid_name?(name) ->
        name

      Token.valid_name?(account.name) ->
        account.name

      is_binary(account.email) ->
        account.email |> String.split("@") |> hd() |> String.slice(0, 80)

      true ->
        "Someone"
    end
  end

  @doc "meet's newest live publishable key (the page's, for guests), or nil."
  def publishable_key do
    Repo.one(
      from(k in Key,
        where: k.app_id == @app and k.mode == "live" and is_nil(k.revoked_at),
        order_by: [desc: k.inserted_at, desc: k.id],
        limit: 1,
        select: k.key
      )
    )
  end
end
