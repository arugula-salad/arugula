defmodule PresenceRelayWeb.StripeWebhook do
  @moduledoc """
  `POST /stripe/webhook`: Stripe's events about subscriptions (#318's
  P11, `PresenceRelay.Billing.handle_event/1`).

  A plug in the endpoint before the body parsers, since the signature is
  over the raw body: it reads the body itself (1 MB at most), checks
  `Stripe-Signature` against `STRIPE_WEBHOOK_SECRET` (and that it was
  signed within 5 minutes), and answers. 200 for an event handled, or
  seen before, or of no interest; 400 for a bad signature or body (or a
  live event on a test relay); 500 when handling failed, so Stripe tries
  again; 404 with billing off.
  """
  @behaviour Plug
  import Plug.Conn
  require Logger

  alias PresenceRelay.Billing
  alias PresenceRelay.Billing.Stripe

  @max 1_000_000

  @impl true
  def init(opts), do: opts

  @impl true
  def call(%Plug.Conn{method: "POST", path_info: ["stripe", "webhook"]} = conn, _) do
    conn |> handle() |> halt()
  end

  def call(conn, _), do: conn

  defp handle(conn) do
    with {:on, true} <- {:on, Billing.enabled?()},
         {:ok, body, conn} <- read_body(conn, length: @max),
         [sig | _] <- get_req_header(conn, "stripe-signature") ++ [nil],
         :ok <- Stripe.verify(body, sig, Billing.config().webhook_secret),
         {:ok, %{"id" => _, "type" => type} = event} <- Jason.decode(body),
         :ok <- same_mode(event) do
      case Billing.handle_event(event) do
        {:ok, result} ->
          reply(conn, 200, %{received: true, result: result})

        {:error, why} ->
          Logger.warning("billing: webhook #{type} failed, Stripe will retry: #{inspect(why)}")
          reply(conn, 500, %{error: "couldn't handle it yet"})
      end
    else
      {:on, false} -> reply(conn, 404, %{error: "billing isn't set up on this relay"})
      {:more, _, conn} -> reply(conn, 413, %{error: "too big"})
      {:error, why} when is_binary(why) -> reply(conn, 400, %{error: why})
      _ -> reply(conn, 400, %{error: "not a Stripe event"})
    end
  end

  # A test-mode relay takes test events only (and a live one, live).
  defp same_mode(%{"livemode" => live}) do
    if live == not Billing.test_mode?(), do: :ok, else: {:error, "wrong mode (test vs live)"}
  end

  defp same_mode(_), do: :ok

  defp reply(conn, status, body) do
    conn
    |> put_resp_content_type("application/json")
    |> send_resp(status, Jason.encode!(body))
  end
end
