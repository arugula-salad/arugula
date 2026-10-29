defmodule PresenceRelay.Apps do
  @moduledoc """
  The host apps this relay serves: each one's id and the HS256 secret its
  server signs room tokens with (`PRESENCE_APPS`). A demo relay (`DEMO=1`)
  also has app `"demo"`, with a secret made at boot, for which
  `POST /api/demo/token` hands out tokens to anyone.
  """

  @key {__MODULE__, :apps}

  @doc "Reads the apps from config, adding the demo app if the demo is on."
  def load do
    apps = Application.get_env(:presence_relay, :apps, %{})

    apps =
      if demo?(),
        do:
          Map.put(apps, "demo", Base.url_encode64(:crypto.strong_rand_bytes(32), padding: false)),
        else: apps

    :persistent_term.put(@key, apps)
  end

  @doc "The app's secret, or nil for an app this relay doesn't know."
  def secret(app) when is_binary(app), do: Map.get(:persistent_term.get(@key, %{}), app)
  def secret(_), do: nil

  def demo?, do: Application.get_env(:presence_relay, :demo, false) == true
end
