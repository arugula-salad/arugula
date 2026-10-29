defmodule PresenceRelay.Billing.Setup do
  @moduledoc """
  Makes what billing needs in a Stripe account (#318's P11), from
  `PresenceRelay.Billing.Plans`: the two meters (extra active users and
  call minutes, each summing its events' `value` per customer, found by
  `stripe_customer_id`) and Team's three prices (the base, licensed and
  monthly; the two metered ones on those meters), each with a lookup key.
  Each product carries tax code `txcd_10103001` (SaaS, business use), as
  Managed Payments needs. Then it checks Stripe takes a Checkout Session
  for Team as the relay makes one (`check_checkout/4`).
  `scripts/stripe_setup.exs` runs it and prints the ids the relay needs.

  Run it again at will: a meter with the event name, or a price with the
  lookup key, the same amount and meter, is kept; a price whose amount
  changed in `Plans` is made anew and takes the lookup key over.

  It asks through `call`, `(method, path, params) -> {:ok, map} |
  {:error, why}` (the relay's own client with a key,
  `PresenceRelay.Billing.Stripe.request/5`, or a fake), and stops at
  once in live mode unless `live: true`.
  """

  alias PresenceRelay.Billing.Plans

  @doc """
  `{:ok, %{meters: %{"mau" => id, ...}, prices: [%{env, id, ...}]}}`, or
  `{:error, why}`. `log` hears what it does.
  """
  def run(call, opts \\ []) do
    log = Keyword.get(opts, :log, fn _ -> :ok end)

    with :ok <- mode(call, Keyword.get(opts, :live, false)),
         {:ok, meters} <- meters(call, log),
         {:ok, prices} <- prices(call, meters, log),
         {:ok, portal} <- portal(call, log) do
      check =
        if Keyword.get(opts, :check_checkout, true),
          do: check_checkout(call, prices, Keyword.get(opts, :managed_payments, true), log),
          else: :skipped

      {:ok, %{meters: meters, prices: prices, portal: portal, checkout: check}}
    end
  end

  # The Customer Portal's settings (Manage): a card, invoices, cancel at
  # the end of the month, name, address and email. Kept if the account has
  # a default one already (set in Stripe's dashboard, say).
  defp portal(call, log) do
    with {:ok, %{"data" => configs}} <-
           call.(:get, "/v1/billing_portal/configurations", %{active: true, limit: 100}) do
      case Enum.find(configs, & &1["is_default"]) do
        %{"id" => id} ->
          log.("portal: #{id} (kept, the default)")
          {:ok, %{id: id, default: true}}

        nil ->
          params = %{
            business_profile: %{
              headline: "arugula.stream",
              privacy_policy_url: "https://arugula.stream/privacy",
              terms_of_service_url: "https://arugula.stream/terms"
            },
            features: %{
              invoice_history: %{enabled: true},
              payment_method_update: %{enabled: true},
              subscription_cancel: %{enabled: true, mode: "at_period_end"},
              customer_update: %{enabled: true, allowed_updates: ~w(email name address)}
            }
          }

          case call.(:post, "/v1/billing_portal/configurations", params) do
            {:ok, %{"id" => id} = c} ->
              log.("portal: #{id} (made; default: #{c["is_default"] == true})")
              {:ok, %{id: id, default: c["is_default"] == true}}

            {:error, e} ->
              {:error, e}
          end
      end
    end
  end

  @doc """
  Whether Stripe takes a Checkout Session for Team as the relay makes
  one (the three prices, two of them metered; Managed Payments on), made
  and expired at once: `:ok` or `{:error, why}`. Nothing in Stripe's
  Managed Payments docs says metered prices are or aren't supported;
  this asks the account itself.
  """
  def check_checkout(call, prices, managed?, log \\ fn _ -> :ok end) do
    [base | metered] = Enum.map(prices, & &1.id)

    params = %{
      mode: "subscription",
      line_items: [%{price: base, quantity: 1} | Enum.map(metered, &%{price: &1})],
      success_url: "https://arugula.stream/dashboard",
      managed_payments: if(managed?, do: %{enabled: true})
    }

    case call.(:post, "/v1/checkout/sessions", params) do
      {:ok, %{"id" => id} = s} ->
        _ = call.(:post, "/v1/checkout/sessions/#{id}/expire", %{})
        mp = get_in(s, ["managed_payments", "enabled"])

        log.(
          "checkout: Stripe took Team's three prices (two metered)" <>
            if(managed?, do: ", Managed Payments #{inspect(mp)}", else: "") <> "; session expired"
        )

        :ok

      {:error, e} ->
        why = if is_binary(e), do: e, else: PresenceRelay.Billing.Stripe.describe(e)
        log.("checkout: Stripe refused Team's prices: " <> why)
        {:error, why}
    end
  end

  # Test mode, unless asked for live.
  defp mode(call, live?) do
    case call.(:get, "/v1/balance", %{}) do
      {:ok, %{"livemode" => true}} when not live? ->
        {:error,
         "this is a LIVE Stripe account: billing is test mode only (STRIPE_LIVE=1 to override)"}

      {:ok, _} ->
        :ok

      {:error, e} ->
        {:error, e}
    end
  end

  defp meters(call, log) do
    with {:ok, %{"data" => existing}} <-
           call.(:get, "/v1/billing/meters", %{status: "active", limit: 100}) do
      Enum.reduce_while(Plans.meters(), {:ok, %{}}, fn {key, m}, {:ok, acc} ->
        case Enum.find(existing, &(&1["event_name"] == m.event_name)) do
          %{"id" => id} ->
            log.("meter #{m.event_name}: #{id} (kept)")
            {:cont, {:ok, Map.put(acc, key, id)}}

          nil ->
            params = %{
              display_name: m.name,
              event_name: m.event_name,
              default_aggregation: %{formula: "sum"},
              customer_mapping: %{type: "by_id", event_payload_key: "stripe_customer_id"},
              value_settings: %{event_payload_key: "value"}
            }

            case call.(:post, "/v1/billing/meters", params) do
              {:ok, %{"id" => id}} ->
                log.("meter #{m.event_name}: #{id} (made)")
                {:cont, {:ok, Map.put(acc, key, id)}}

              {:error, e} ->
                {:halt, {:error, e}}
            end
        end
      end)
    end
  end

  defp prices(call, meters, log) do
    Enum.reduce_while(Plans.stripe_prices(), {:ok, []}, fn p, {:ok, acc} ->
      meter = p.meter && Map.fetch!(meters, p.meter)

      found =
        case call.(:get, "/v1/prices", %{lookup_keys: [p.lookup_key], active: true, limit: 1}) do
          {:ok, %{"data" => [price | _]}} -> if same?(price, p, meter), do: price
          _ -> nil
        end

      result =
        if found do
          log.("price #{p.lookup_key}: #{found["id"]} (kept)")
          # Its product carries the tax code (Managed Payments needs it).
          with {:ok, _} <- tax_code(call, found["product"]), do: {:ok, found["id"]}
        else
          recurring =
            if meter,
              do: %{interval: "month", usage_type: "metered", meter: meter},
              else: %{interval: "month", usage_type: "licensed"}

          params = %{
            currency: Plans.currency(),
            unit_amount_decimal: p.cents,
            recurring: recurring,
            product_data: %{name: p.product, tax_code: Plans.tax_code()},
            lookup_key: p.lookup_key,
            transfer_lookup_key: true,
            nickname: p.product
          }

          case call.(:post, "/v1/prices", params) do
            {:ok, %{"id" => id}} ->
              log.("price #{p.lookup_key}: #{id} (made, #{p.cents}¢)")
              {:ok, id}

            {:error, e} ->
              {:error, e}
          end
        end

      case result do
        {:ok, id} -> {:cont, {:ok, acc ++ [Map.put(p, :id, id)]}}
        {:error, e} -> {:halt, {:error, e}}
      end
    end)
  end

  defp tax_code(call, %{"id" => id}), do: tax_code(call, id)

  defp tax_code(call, product) when is_binary(product),
    do: call.(:post, "/v1/products/#{product}", %{tax_code: Plans.tax_code()})

  defp tax_code(_, _), do: {:ok, nil}

  defp same?(price, p, meter) do
    amount = price["unit_amount_decimal"] && Decimal.new(price["unit_amount_decimal"])

    amount != nil and Decimal.equal?(amount, Decimal.new(p.cents)) and
      price["currency"] == Plans.currency() and
      get_in(price, ["recurring", "meter"]) == meter
  end
end
