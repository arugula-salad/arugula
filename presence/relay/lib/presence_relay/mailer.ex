defmodule PresenceRelay.Mailer do
  @moduledoc """
  Email (magic links). Production sends through Resend when
  `RESEND_API_KEY` is set (runtime.exs), from `MAIL_FROM`; without it
  there's no sender, and email sign-in says it isn't set up rather than
  logging live links (guests and Google still work). Dev logs mail;
  tests use Swoosh's test adapter.
  """
  use Swoosh.Mailer, otp_app: :presence_relay

  @doc "Whether mail can be sent here."
  def ready?,
    do: Application.get_env(:presence_relay, __MODULE__, [])[:adapter] not in [nil, false]
end
