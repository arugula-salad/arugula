defmodule PresenceRelayWeb.Endpoint do
  use Phoenix.Endpoint, otp_app: :presence_relay

  @session Application.compile_env(:presence_relay, :session)

  # Host apps embed the client on their own origins: a room token admits a
  # socket from anywhere; a publishable key only from its app's origins,
  # which the socket checks with the origin PresenceRelayWeb.Origin notes.
  socket "/socket", PresenceRelayWeb.Socket,
    websocket: [check_origin: {PresenceRelayWeb.Origin, :note, []}],
    longpoll: false

  # The SDK, its models and demo tokens, for pages on other origins.
  plug PresenceRelayWeb.Cors

  # The demo site (step 3 builds it into priv/static) and the SDK bundle
  # (priv/static/sdk/v0).
  plug Plug.Static, at: "/", from: :presence_relay, gzip: false

  plug Plug.RequestId
  # Stripe's webhooks (POST /stripe/webhook): signed over the raw body, so
  # before the parsers.
  plug PresenceRelayWeb.StripeWebhook
  # JSON for the API; a form's encoding for the sign-in page's button
  # (its CSRF token comes in the body).
  plug Plug.Parsers, parsers: [:urlencoded, :json], pass: ["*/*"], json_decoder: Jason
  plug Plug.Session, @session
  plug PresenceRelayWeb.Router
end
