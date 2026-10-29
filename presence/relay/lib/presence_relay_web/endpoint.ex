defmodule PresenceRelayWeb.Endpoint do
  use Phoenix.Endpoint, otp_app: :presence_relay

  # Host apps embed the client on their own origins: the room token, not
  # the origin, is what admits a socket.
  socket "/socket", PresenceRelayWeb.Socket, websocket: [check_origin: false], longpoll: false

  # The demo site (step 3 builds it into priv/static).
  plug Plug.Static, at: "/", from: :presence_relay, gzip: false

  plug Plug.RequestId
  plug Plug.Parsers, parsers: [:json], pass: ["*/*"], json_decoder: Jason
  plug PresenceRelayWeb.Router
end
