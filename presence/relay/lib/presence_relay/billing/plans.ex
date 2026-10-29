defmodule PresenceRelay.Billing.Plans do
  @moduledoc """
  Every price and allowance arugula.stream bills by (#318's P11), in one
  place. The relay's caps, the dashboard's estimate and the Stripe setup
  script (`scripts/stripe_setup.exs`, which makes the prices from these)
  all read them here.

  > #### Placeholders {: .warning}
  >
  > Jake hasn't set the numbers. Every figure marked PLACEHOLDER below is
  > a guess from the /hosted pitch ("about $29/month, priced by the people
  > who use it"), to be confirmed before going live. Changing a price
  > means running the setup script again (it makes new Stripe prices and
  > prints their ids) and setting the new ids.

  - **Starter:** free, with a hard cap on monthly active users.
  - **Team:** a monthly base price that includes some monthly active
    users; each one past that, and each call minute, billed by Stripe's
    meters at the end of the month.
  - **Enterprise:** no self-serve; a "talk to us" link.

  Money is in US dollars, as decimal strings (Stripe takes fractions of
  a cent per unit).
  """

  # PLACEHOLDER: the most monthly active users a Starter app may have.
  @starter_mau_cap 100

  # PLACEHOLDER: Team's base price a month, the monthly active users it
  # includes, and the price of each one past those and of each call minute.
  @team_base "29.00"
  @team_included_mau 500
  @team_per_extra_mau "0.05"
  @team_per_call_minute "0.004"

  # How long a Team app keeps Team's caps once its payments stop (Stripe
  # says past_due or unpaid): a week, while Stripe retries the card and
  # the owner fixes it. Then Starter's cap, until it's paid.
  @grace_days 7

  @currency "usd"
  # The products' tax code, as Managed Payments (Stripe as merchant of
  # record) requires: software as a service, business use.
  @tax_code "txcd_10103001"
  @enterprise "mailto:hello@arugula.stream"

  # Stripe's meters, by the event name meter events carry: extra monthly
  # active users (past Team's included ones) and call minutes.
  @meters %{
    "mau" => %{event_name: "presence_extra_active_users", name: "Extra active users"},
    "minutes" => %{event_name: "presence_call_minutes", name: "Call minutes"}
  }

  @doc "Starter's cap on monthly active users (config `:starter_mau_cap` in tests)."
  def starter_mau_cap,
    do: Application.get_env(:presence_relay, :starter_mau_cap, @starter_mau_cap)

  def team_base, do: Decimal.new(@team_base)
  def team_included_mau, do: @team_included_mau
  def team_per_extra_mau, do: Decimal.new(@team_per_extra_mau)
  def team_per_call_minute, do: Decimal.new(@team_per_call_minute)
  def grace_days, do: @grace_days
  def currency, do: @currency
  def tax_code, do: @tax_code
  def enterprise_contact, do: @enterprise

  @doc "The meters, `\"mau\"` and `\"minutes\"`, each with its Stripe event name and a name."
  def meters, do: @meters

  def event_name(meter), do: @meters |> Map.fetch!(meter) |> Map.fetch!(:event_name)

  @doc "The plans as the dashboard shows them."
  def view do
    %{
      starter: %{price: "0.00", active_users_cap: starter_mau_cap()},
      team: %{
        base: @team_base,
        included_active_users: @team_included_mau,
        per_extra_active_user: @team_per_extra_mau,
        per_call_minute: @team_per_call_minute,
        currency: @currency
      },
      enterprise: %{contact: @enterprise},
      grace_days: @grace_days
    }
  end

  @doc """
  The three Team prices the setup script makes, by their lookup keys:
  the base (licensed, a month) and the two metered ones. Amounts in
  cents, as Stripe's `unit_amount_decimal`.
  """
  def stripe_prices do
    [
      %{
        lookup_key: "presence_team_base",
        env: "STRIPE_PRICE_TEAM_BASE",
        product: "arugula.stream Team",
        cents: cents(@team_base),
        meter: nil
      },
      %{
        lookup_key: "presence_team_extra_active_users",
        env: "STRIPE_PRICE_TEAM_MAU",
        product: "arugula.stream extra active users",
        cents: cents(@team_per_extra_mau),
        meter: "mau"
      },
      %{
        lookup_key: "presence_team_call_minutes",
        env: "STRIPE_PRICE_TEAM_MINUTES",
        product: "arugula.stream call minutes",
        cents: cents(@team_per_call_minute),
        meter: "minutes"
      }
    ]
  end

  defp cents(dollars),
    do:
      dollars
      |> Decimal.new()
      |> Decimal.mult(100)
      |> Decimal.normalize()
      |> Decimal.to_string(:normal)
end
