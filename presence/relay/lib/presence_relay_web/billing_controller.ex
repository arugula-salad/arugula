defmodule PresenceRelayWeb.BillingController do
  @moduledoc """
  An app's billing (#318's P11, `PresenceRelay.Billing`), for its signed-in
  owner: the dashboard's billing card, and the way to Stripe's Checkout
  (Upgrade) and Customer Portal (Manage), each a URL the page goes to.
  """
  use Phoenix.Controller, formats: [:json]

  alias PresenceRelay.{Apps, Billing}
  alias PresenceRelayWeb.Auth

  @doc "The plan, the subscription, the cap, the plans and this month's cost so far."
  def show(conn, %{"id" => id}), do: with_app(conn, id, &json(conn, Billing.view(&1)))

  @doc "`{url}`: a Stripe Checkout page subscribing the app to Team."
  def checkout(conn, %{"id" => id}) do
    with_app(conn, id, fn app -> answer(conn, Billing.checkout(app, Auth.base_url(conn))) end)
  end

  @doc "`{url}`: the Stripe Customer Portal for the app's subscription."
  def portal(conn, %{"id" => id}) do
    with_app(conn, id, fn app -> answer(conn, Billing.portal(app, Auth.base_url(conn))) end)
  end

  defp answer(conn, {:ok, url}), do: json(conn, %{url: url})

  defp answer(conn, {:error, status, msg}),
    do: conn |> put_status(status) |> json(%{error: msg})

  defp with_app(conn, id, fun) do
    case Apps.get(conn.assigns.account, id) do
      nil -> conn |> put_status(404) |> json(%{error: "no such app"})
      app -> fun.(app)
    end
  end
end
