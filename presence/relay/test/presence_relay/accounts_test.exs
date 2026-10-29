defmodule PresenceRelay.AccountsTest do
  use PresenceRelay.DataCase, async: false
  import Swoosh.TestAssertions

  alias PresenceRelay.Accounts

  defp token_from_email do
    assert_email_sent(fn email ->
      [_, token] = Regex.run(~r{/auth/email/([A-Za-z0-9_-]+)}, email.text_body)
      send(self(), {:token, token})
      true
    end)

    assert_received {:token, token}
    token
  end

  test "a magic link signs its address in once, making the account the first time" do
    assert :ok = Accounts.send_link("Ada@Example.com", "https://arugula.stream")
    token = token_from_email()
    assert {:ok, a} = Accounts.use_link(token)
    assert a.email == "ada@example.com"
    assert {:error, :used} = Accounts.use_link(token)

    :ok = Accounts.send_link("ada@example.com", "https://arugula.stream")
    assert {:ok, again} = Accounts.use_link(token_from_email())
    assert again.id == a.id
  end

  test "an expired link doesn't work, nor a made-up one, nor a bad address" do
    :ok = Accounts.send_link("ada@example.com", "https://x")
    token = token_from_email()
    Repo.update_all(Accounts.LoginToken, set: [expires_at: ~U[2020-01-01 00:00:00Z]])
    assert {:error, :expired} = Accounts.use_link(token)
    assert {:error, :unknown} = Accounts.use_link("nope")
    assert {:error, :bad_email} = Accounts.send_link("not an email", "https://x")
  end

  test "without a mail sender, links aren't sent" do
    prev = Application.get_env(:presence_relay, PresenceRelay.Mailer)
    Application.put_env(:presence_relay, PresenceRelay.Mailer, adapter: nil)
    on_exit(fn -> Application.put_env(:presence_relay, PresenceRelay.Mailer, prev) end)
    assert {:error, :no_mailer} = Accounts.send_link("ada@example.com", "https://x")
  end

  test "Google joins the account with the same verified address, and keeps finding it" do
    :ok = Accounts.send_link("ada@example.com", "https://x")
    {:ok, by_email} = Accounts.use_link(token_from_email())

    profile = %{
      "sub" => "g-1",
      "email" => "ada@example.com",
      "email_verified" => true,
      "name" => "Ada"
    }

    assert {:ok, by_google} = Accounts.from_google(profile)
    assert by_google.id == by_email.id
    assert {:ok, again} = Accounts.from_google(%{profile | "email" => "changed@example.com"})
    assert again.id == by_email.id

    # An unverified address doesn't join anyone's account.
    assert {:ok, other} =
             Accounts.from_google(%{
               "sub" => "g-2",
               "email" => "ada@example.com",
               "email_verified" => false
             })

    refute other.id == by_email.id
  end
end
