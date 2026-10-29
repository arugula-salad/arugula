defmodule PresenceRelay.Chat do
  @moduledoc """
  A room's text messages (PROTOCOL.md, `say`, `said`, `history`; #318's
  P5): checked, kept per app and room in order with a stable id, and read
  back newest-last, 50 at a time.

  Kept in Postgres when the relay has a database (`database: true`); a
  relay without one keeps each room's last 50 in memory (`Chat.Memory`),
  so the demo can talk too. Either way a message is
  `%{id, sub, name, text, at}`, `at` in Unix milliseconds.
  """
  import Ecto.Query
  require Logger

  alias PresenceRelay.Chat.{Memory, Message}
  alias PresenceRelay.Repo

  @max_chars 2000
  @page 50

  @type message :: %{
          id: integer,
          sub: String.t(),
          name: String.t(),
          text: String.t(),
          at: integer
        }

  @doc "The page size: a joiner's history, and each `before` page."
  def page, do: @page

  @doc """
  `text` as it would be kept, trimmed, or why it can't be: `:empty`,
  `:too_long` (over 2000 characters) or `:control` (a control character
  other than a newline).
  """
  def check(text) when is_binary(text) do
    t = String.trim(text)

    cond do
      not String.valid?(t) -> {:error, :control}
      t == "" -> {:error, :empty}
      String.length(t) > @max_chars -> {:error, :too_long}
      String.match?(t, ~r/[\x00-\x09\x0B-\x1F\x7F]/u) -> {:error, :control}
      true -> {:ok, t}
    end
  end

  def check(_), do: {:error, :empty}

  @doc "Keeps `sub`'s message in the room: `{:ok, message}` or `{:error, reason}`."
  def say(app, room, sub, name, text) do
    with {:ok, t} <- check(text) do
      if database?(),
        do: insert(app, room, sub, name, t),
        else: {:ok, Memory.put(app, room, %{sub: sub, name: name, text: t})}
    end
  end

  @doc """
  The room's messages oldest first: the last #{@page}, or the #{@page}
  before message `before`.
  """
  def history(app, room, before \\ nil) do
    if database?(), do: query(app, room, before), else: Memory.history(app, room, before, @page)
  end

  defp insert(app, room, sub, name, text) do
    %Message{}
    |> Ecto.Changeset.change(app: app, room: room, sub: sub, name: name, text: text)
    |> Repo.insert()
    |> case do
      {:ok, m} -> {:ok, view(m)}
      {:error, _} -> {:error, :not_kept}
    end
  rescue
    # Postgres slow or away (the pool's queue gave up): the message isn't
    # kept and the sender hears so; the room stays up (#318's P12 found a
    # busy pool crashing rooms' channels here).
    e in [DBConnection.ConnectionError, Postgrex.Error] ->
      Logger.warning("chat: a message wasn't kept: #{Exception.message(e)}")
      {:error, :not_kept}
  end

  defp query(app, room, before) do
    q =
      from(m in Message,
        where: m.app == ^app and m.room == ^room,
        order_by: [desc: m.id],
        limit: @page
      )

    q = if is_integer(before), do: where(q, [m], m.id < ^before), else: q
    q |> Repo.all() |> Enum.reverse() |> Enum.map(&view/1)
  rescue
    # Likewise: no history this time rather than no room.
    e in [DBConnection.ConnectionError, Postgrex.Error] ->
      Logger.warning("chat: no history: #{Exception.message(e)}")
      []
  end

  defp view(%Message{} = m),
    do: %{
      id: m.id,
      sub: m.sub,
      name: m.name,
      text: m.text,
      at: DateTime.to_unix(m.inserted_at, :millisecond)
    }

  defp database?, do: Application.get_env(:presence_relay, :database, true)
end
