defmodule PresenceRelay.LogFormatter do
  @moduledoc """
  Production's log lines (#318's P12): one JSON object a line, for Fly's
  log search, with the time, level, message, the module that logged it,
  and the request id and app id where there are ones; no other metadata.

  Nothing a log line says may carry a token, an email address or a
  secret: every message goes through `scrub/1` first, which blanks
  email addresses, JWTs (room tokens, provider tokens, LiveKit tokens),
  publishable and secret keys, the magic link's token in its path, the
  values of query parameters that carry any of those, bearer tokens, and
  the password in a database URL. Phoenix's own lines already filter the
  same parameters (`:filter_parameters`, config/config.exs).

  config/prod.exs makes it the default handler's formatter.
  """

  @text elem(Logger.Formatter.new(format: "$message", metadata: [], colors: [enabled: false]), 1)

  @scrubs [
    # A database URL's password.
    {~r{(\b[a-z][a-z0-9+.-]*://[^:/@\s]+:)[^@\s]+@}i, "\\1[filtered]@"},
    # The magic link's token, in its path.
    {~r{(/auth/email/)[^\s/?"'&]+}, "\\1[token]"},
    # JWTs: three base64url parts, the first a JSON header ("eyJ").
    {~r/\beyJ[A-Za-z0-9_-]{5,}\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]*/, "[token]"},
    # Publishable and secret keys, Stripe-style and Resend's.
    {~r/\b(?:pk|sk|rk|whsec)_(?:(?:live|test)_)?[A-Za-z0-9_-]{6,}/, "[key]"},
    {~r/\bre_[A-Za-z0-9_]{16,}/, "[key]"},
    # Bearer tokens.
    {~r/(\bBearer\s+)\S+/i, "\\1[token]"},
    # Query parameters and key=value pairs that carry a credential.
    {~r/\b((?:token|auth|key|secret|password|code|state|gid|api_key)=)[^&\s"']+/i,
     "\\1[filtered]"},
    # Email addresses.
    {~r/[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}/, "[email]"}
  ]

  @doc "`text` with tokens, email addresses and secrets blanked."
  def scrub(text) when is_binary(text),
    do: Enum.reduce(@scrubs, text, fn {re, with}, t -> Regex.replace(re, t, with) end)

  @doc false
  # The `:logger` formatter callback: one JSON line.
  def format(%{level: level, meta: meta} = event, _config) do
    msg = event |> Logger.Formatter.format(@text) |> IO.chardata_to_string() |> scrub()

    line =
      %{time: time(meta), level: level, msg: msg}
      |> put(:source, source(meta))
      |> put(:request_id, meta[:request_id])
      |> put(:app, meta[:app] && scrub(to_string(meta[:app])))

    [Jason.encode_to_iodata!(line), ?\n]
  rescue
    # A line that can't be formatted still says something.
    e ->
      ["{\"level\":\"error\",\"msg\":\"log line unformattable: ", inspect(e.__struct__), "\"}\n"]
  end

  @doc false
  def check_config(_config), do: :ok

  defp time(%{time: t}) when is_integer(t),
    do: t |> DateTime.from_unix!(:microsecond) |> DateTime.to_iso8601()

  defp time(_), do: DateTime.utc_now() |> DateTime.to_iso8601()

  defp source(%{mfa: {m, f, a}}), do: "#{inspect(m)}.#{f}/#{a}"
  defp source(_), do: nil

  defp put(map, _key, nil), do: map
  defp put(map, key, value), do: Map.put(map, key, value)
end
