defmodule PresenceRelay.FakeGoogle do
  @moduledoc "Google's token exchange, faked: code `good` is Ada; anything else fails."
  @behaviour PresenceRelay.Accounts.Google

  @impl true
  def exchange("good", _redirect, _config),
    do:
      {:ok,
       %{
         "sub" => "google-ada",
         "email" => "ada@example.com",
         "email_verified" => true,
         "name" => "Ada"
       }}

  def exchange(_, _, _), do: {:error, :bad_code}
end
