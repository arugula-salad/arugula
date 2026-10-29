defmodule PresenceRelay.Repo do
  @moduledoc "Postgres: accounts, apps, messages and usage (#318). Never frames."
  use Ecto.Repo, otp_app: :presence_relay, adapter: Ecto.Adapters.Postgres
end
