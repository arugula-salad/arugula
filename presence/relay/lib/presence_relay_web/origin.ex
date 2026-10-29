defmodule PresenceRelayWeb.Origin do
  @moduledoc """
  The page's Origin, for a socket admitted by a publishable key.

  Phoenix's `check_origin` hook sees the Origin but not the socket's
  params, and `connect/3` sees the params but not the Origin. Both run in
  the same process, one straight after the other (Phoenix.Transports.
  WebSocket.call/2), so the hook notes the Origin here and `connect/3`
  takes it (and clears it: a kept-alive connection's next request must
  not see a stale one). The hook allows every origin itself: a room token
  doesn't care where the page is, and a key's origins are checked in
  `connect/3`.
  """

  @key {__MODULE__, :origin}

  @doc "The socket's check_origin hook: notes the origin, allows it."
  def note(%URI{} = uri) do
    Process.put(@key, origin(uri))
    true
  end

  @doc "The noted origin (`scheme://host[:port]`), or nil; clears it."
  def take, do: Process.delete(@key)

  defp origin(%URI{scheme: s, host: h, port: p}) do
    default = if s == "https", do: 443, else: 80
    if p && p != default, do: "#{s}://#{h}:#{p}", else: "#{s}://#{h}"
  end
end
