defmodule PresenceRelay.Presence do
  @moduledoc """
  Who's in a room (PROTOCOL.md, "Presence"): a meta per connection, keyed
  by the member's `sub`, tracked on the room's internal topic
  (`p:<app>:<room>`, so two apps' rooms of one name never meet).
  """
  use Phoenix.Presence, otp_app: :presence_relay, pubsub_server: PresenceRelay.PubSub
end
