defmodule PresenceRelayWeb.Router do
  use Phoenix.Router

  import PresenceRelayWeb.Auth, only: [fetch_account: 2, require_account: 2, json_writes: 2]

  pipeline :api do
    plug :accepts, ["json"]
  end

  # Sign-in pages: the browser follows links and redirects here. Their
  # forms carry a CSRF token.
  pipeline :browser do
    plug :fetch_session
    plug :protect_from_forgery
    plug :fetch_account
  end

  # The dashboard's and meet's JSON, by session cookie (SameSite=Lax; a
  # write must be JSON, which a cross-site form can't send).
  pipeline :api_v1 do
    plug :accepts, ["json"]
    plug :fetch_session
    plug :fetch_account
    plug :json_writes
  end

  pipeline :signed_in do
    plug :require_account
  end

  get "/healthz", PresenceRelayWeb.PageController, :healthz

  scope "/api", PresenceRelayWeb do
    pipe_through :api
    post "/demo/token", DemoController, :token
  end

  scope "/auth", PresenceRelayWeb do
    pipe_through :browser
    get "/email/:token", AuthController, :email_confirm
    post "/email/:token", AuthController, :email_callback
    get "/google", AuthController, :google
    get "/google/callback", AuthController, :google_callback
  end

  scope "/api/v1", PresenceRelayWeb do
    pipe_through :api_v1
    post "/auth/email", AuthController, :email_request
    post "/auth/logout", AuthController, :logout
    get "/auth/providers", AuthController, :providers
    get "/me", MeController, :show
    get "/meet/session", MeetController, :session
    post "/meet/rooms", MeetController, :create
    get "/meet/rooms/:slug", MeetController, :show
  end

  scope "/api/v1", PresenceRelayWeb do
    pipe_through [:api_v1, :signed_in]
    get "/apps", AppsController, :index
    post "/apps", AppsController, :create
    get "/apps/:id", AppsController, :show
    patch "/apps/:id", AppsController, :update
    post "/apps/:id/secret", AppsController, :rotate_secret
    post "/apps/:id/origins", AppsController, :add_origin
    delete "/apps/:id/origins", AppsController, :remove_origin
    post "/apps/:id/keys", AppsController, :make_key
    delete "/apps/:id/keys/:key", AppsController, :revoke_key
    get "/apps/:id/issuer", AppsController, :issuer
    put "/apps/:id/issuer", AppsController, :put_issuer
    delete "/apps/:id/issuer", AppsController, :delete_issuer
    get "/meet/rooms", MeetController, :rooms
    post "/meet/rooms/:slug/token", MeetController, :token
  end

  # Anything else is a single-page app's: meet's on a meet. host or under
  # /meet, the dashboard's under /dashboard, else the demo site's
  # (PageController.index).
  get "/*path", PresenceRelayWeb.PageController, :index
end
