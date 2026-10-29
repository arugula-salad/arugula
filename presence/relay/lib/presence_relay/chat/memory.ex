defmodule PresenceRelay.Chat.Memory do
  @moduledoc """
  A relay without a database keeps each room's last #{50} messages here
  (ETS, owned by this process), so text works on the demo. Ids only go up,
  on this node.
  """
  use GenServer

  @table :presence_relay_messages
  @keep 50

  def start_link(_), do: GenServer.start_link(__MODULE__, nil, name: __MODULE__)

  @impl true
  def init(nil) do
    :ets.new(@table, [:named_table, :public, :ordered_set, read_concurrency: true])
    {:ok, nil}
  end

  @doc "Keeps `msg` (sub, name, text) in the room, dropping the oldest past #{@keep}."
  def put(app, room, msg) do
    id = System.unique_integer([:positive, :monotonic])
    m = Map.merge(msg, %{id: id, at: System.system_time(:millisecond)})
    :ets.insert(@table, {{app, room, id}, m})
    trim(app, room)
    m
  end

  @doc "The room's messages oldest first: the last `n`, or the `n` before `before`."
  def history(app, room, before, n) do
    top = if is_integer(before), do: before, else: :infinity

    :ets.select(@table, [{{{app, room, :"$1"}, :"$2"}, [{:<, :"$1", top}], [:"$2"]}])
    |> Enum.take(-n)
  end

  defp trim(app, room) do
    ids = :ets.select(@table, [{{{app, room, :"$1"}, :_}, [], [:"$1"]}])
    for id <- Enum.drop(ids, -@keep), do: :ets.delete(@table, {app, room, id})
  end
end
