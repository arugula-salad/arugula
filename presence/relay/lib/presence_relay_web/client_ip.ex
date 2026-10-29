defmodule PresenceRelayWeb.ClientIP do
  @moduledoc """
  The client's IP, for the abuse limits (`PresenceRelay.Limits`).

  On Fly, every request comes through Fly's proxy, so the peer address is
  the proxy's; the client's is in `Fly-Client-IP`, which the proxy sets.
  Off Fly anyone can send that header, so it's trusted only where config
  `:trust_fly_client_ip` says (config/runtime.exs: when `FLY_APP_NAME` is
  set, which Fly sets on its machines); otherwise, and when the header
  isn't an address, the peer address is the client's.

  A socket's `connect/3` never sees the request's headers (Phoenix hands
  it only its params and `connect_info`), so the endpoint notes the
  client IP of a socket request before Phoenix dispatches it (see
  `__before_compile__/1`), and `connect/3` takes it, as
  `PresenceRelayWeb.Origin` does the Origin: the same process, one
  straight after the other.
  """

  @key {__MODULE__, :ip}

  @doc "The client IP of `conn`, as a string (`\"203.0.113.9\"`, `\"2001:db8::1\"`)."
  def of(%Plug.Conn{} = conn) do
    header =
      if Application.get_env(:presence_relay, :trust_fly_client_ip, false),
        do: conn |> Plug.Conn.get_req_header("fly-client-ip") |> List.first()

    parse(header) || format(conn.remote_ip)
  end

  @doc false
  # The endpoint's note, for a socket request: see the moduledoc.
  def note(%Plug.Conn{path_info: ["socket" | _]} = conn), do: Process.put(@key, of(conn))
  def note(_conn), do: :ok

  @doc """
  A socket's client IP: the noted one (and clears it, so a kept-alive
  connection's next request can't see it), else `connect_info`'s peer
  address (tests), else nil.
  """
  def take(info \\ %{}) do
    case Process.delete(@key) do
      nil ->
        case info do
          %{peer_data: %{address: address}} -> format(address)
          _ -> nil
        end

      ip ->
        ip
    end
  end

  defp parse(nil), do: nil

  defp parse(s) do
    case :inet.parse_strict_address(String.to_charlist(String.trim(s))) do
      {:ok, ip} -> format(ip)
      _ -> nil
    end
  end

  defp format(ip) when is_tuple(ip), do: ip |> :inet.ntoa() |> to_string()
  defp format(_), do: nil

  @doc false
  # `@before_compile` on the endpoint, after Phoenix's own: its `call/2`
  # notes a socket request's client IP, then does what it did.
  defmacro __before_compile__(_env) do
    quote do
      defoverridable call: 2

      def call(conn, opts) do
        PresenceRelayWeb.ClientIP.note(conn)
        super(conn, opts)
      end
    end
  end
end
