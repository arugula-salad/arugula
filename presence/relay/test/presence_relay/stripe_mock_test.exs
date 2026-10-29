defmodule PresenceRelay.StripeMockTest do
  # The relay's Stripe requests against stripe-mock (#318's P11), which
  # checks each against Stripe's OpenAPI spec: a parameter Stripe doesn't
  # take, or a shape it doesn't, is a 400. Customers, Checkout (Managed
  # Payments on), the portal, meter events and the setup script's meters,
  # prices and products. Runs where STRIPE_MOCK_URL is set (CI's
  # stripe-mock service; locally `docker run -p 12111:12111
  # stripe/stripe-mock`).
  use PresenceRelay.DataCase, async: false

  alias PresenceRelay.{Accounts, Apps, Billing}
  alias PresenceRelay.Accounts.Account
  alias PresenceRelay.Billing.{MeterEvent, Plans, Reporter, Setup, Stripe}

  @moduletag :stripe_mock
  @moduletag :capture_log

  setup do
    config = %{
      secret_key: "sk_test_123mock",
      webhook_secret: "whsec_mock",
      prices: %{base: "price_base", mau: "price_mau", minutes: "price_minutes"},
      api_base: System.get_env("STRIPE_MOCK_URL", "http://127.0.0.1:12111"),
      live: false
    }

    Application.put_env(:presence_relay, :stripe, config)
    on_exit(fn -> Application.put_env(:presence_relay, :stripe, nil) end)

    owner =
      Repo.insert!(%Account{
        id: "acc_sm#{System.unique_integer([:positive])}",
        email: "sm@example.com"
      })

    {:ok, app, _} = Apps.create(owner, %{"name" => "Mocked"})
    %{config: Billing.config(), app: app, owner: owner}
  end

  test "an unknown parameter is refused: stripe-mock is checking", %{config: c} do
    assert {:error, {400, _}} = Stripe.request(c, :post, "/v1/customers", %{no_such_param: "x"})
  end

  test "Checkout with Managed Payments, then the portal", %{app: app} do
    assert {:ok, "https://" <> _} = Billing.checkout(app, "https://arugula.stream")
    assert Billing.get(app.id).customer_id =~ ~r/\Acus_/
    assert {:ok, "https://" <> _} = Billing.portal(app, "https://arugula.stream")
    assert Accounts.get(app.owner_id)
  end

  test "a meter event as the reporter sends it", %{config: c, app: app} do
    Repo.insert!(%MeterEvent{
      identifier: "#{app.id}:2026-10:minutes:5",
      app_id: app.id,
      customer_id: "cus_mock",
      event_name: Plans.event_name("minutes"),
      value: 5,
      timestamp: DateTime.utc_now() |> DateTime.truncate(:second)
    })

    assert {1, 0} = Reporter.send_pending(c, DateTime.utc_now())
  end

  test "the setup script's meters, prices (with their products' tax code) and Checkout check", %{
    config: c
  } do
    call = fn method, path, params -> Stripe.request(c, method, path, params) end
    lines = :ets.new(:lines, [:public, :bag])
    log = &:ets.insert(lines, {&1})

    assert {:ok, %{meters: meters, prices: prices, checkout: :ok}} = Setup.run(call, log: log)
    assert Map.keys(meters) == ["mau", "minutes"]

    assert Enum.map(prices, & &1.env) ==
             ~w(STRIPE_PRICE_TEAM_BASE STRIPE_PRICE_TEAM_MAU STRIPE_PRICE_TEAM_MINUTES)

    assert Enum.all?(prices, &(&1.id =~ ~r/\Aprice_/))

    # stripe-mock answers every list with something, so the meters it
    # found aren't ours: the made ones (create) were checked by the
    # price creates that name them; a meter create is checked here.
    params = %{
      display_name: "Call minutes",
      event_name: "presence_call_minutes",
      default_aggregation: %{formula: "sum"},
      customer_mapping: %{type: "by_id", event_payload_key: "stripe_customer_id"},
      value_settings: %{event_payload_key: "value"}
    }

    assert {:ok, %{"object" => "billing.meter"}} =
             Stripe.request(c, :post, "/v1/billing/meters", params)

    assert :ets.tab2list(lines) != []
  end
end
