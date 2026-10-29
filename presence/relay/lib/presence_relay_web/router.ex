defmodule PresenceRelayWeb.Router do
  use Phoenix.Router

  pipeline :api do
    plug :accepts, ["json"]
  end

  get "/healthz", PresenceRelayWeb.PageController, :healthz

  scope "/api", PresenceRelayWeb do
    pipe_through :api
    post "/demo/token", DemoController, :token
  end

  # Anything else is the demo site's (a single-page app): its index.
  get "/*path", PresenceRelayWeb.PageController, :index
end
