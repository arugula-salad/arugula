# Makes billing's meters, Team's prices and a Customer Portal setup in a
# Stripe account (#318's P11; PresenceRelay.Billing.Setup), from
# PresenceRelay.Billing.Plans, and prints the environment the relay needs.
# Test mode only, unless STRIPE_LIVE=1. Safe to run again: what's there
# already is kept.
#
# The account is the key's (from the environment, never argv):
#
#   STRIPE_SECRET_KEY=sk_test_… mise exec -- mix run --no-start scripts/stripe_setup.exs > prices.env
#
# STRIPE_API_BASE points it elsewhere (stripe-mock). Last, it makes (and
# expires) a Team Checkout Session as the relay makes one, Managed
# Payments on (STRIPE_MANAGED_PAYMENTS=0: off), to see that Stripe takes
# the metered prices that way; it exits 3 if not.

alias PresenceRelay.Billing.{Setup, Stripe}

managed = System.get_env("STRIPE_MANAGED_PAYMENTS") not in ~w(0 false)
live = System.get_env("STRIPE_LIVE") in ~w(1 true)
log = fn line -> IO.puts(:stderr, line) end
key = System.get_env("STRIPE_SECRET_KEY") || ""

cond do
  key == "" ->
    IO.puts(:stderr, "Set STRIPE_SECRET_KEY (sk_test_…) in the environment.")
    System.halt(2)

  String.starts_with?(key, ["sk_live_", "rk_live_"]) and not live ->
    IO.puts(:stderr, "That's a live key: billing is test mode only (STRIPE_LIVE=1 to override).")
    System.halt(2)

  true ->
    :ok
end

{:ok, _} = Application.ensure_all_started(:finch)
{:ok, _} = Finch.start_link(name: PresenceRelay.Finch)

config = %{
  secret_key: key,
  api_base: System.get_env("STRIPE_API_BASE") || "https://api.stripe.com",
  api_version: System.get_env("STRIPE_API_VERSION") || "2026-09-30.endive"
}

call = fn method, path, params -> Stripe.request(config, method, path, params) end

case Setup.run(call, live: live, log: log, managed_payments: managed) do
  {:ok, %{prices: prices, meters: meters, checkout: check}} ->
    IO.puts(:stderr, "\nThe relay's environment (with STRIPE_SECRET_KEY and STRIPE_WEBHOOK_SECRET):\n")
    for p <- prices, do: IO.puts("#{p.env}=#{p.id}")
    IO.puts(:stderr, "\nMeters (for the Stripe dashboard): #{inspect(meters)}")

    if check != :ok do
      IO.puts(:stderr, "\nWARNING: Stripe refused a Team Checkout as the relay makes it (see above).")
      System.halt(3)
    end

  {:error, e} ->
    IO.puts(:stderr, "Setup stopped: " <> if(is_binary(e), do: e, else: Stripe.describe(e)))
    System.halt(1)
end
