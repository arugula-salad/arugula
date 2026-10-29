defmodule PresenceRelay.FakeStripe do
  @moduledoc """
  The little of Stripe billing uses (#318's P11), faked on 127.0.0.1 for
  the webhook and meter flows: customers, Checkout and portal sessions,
  subscriptions (a test sets them, as Stripe would after a Checkout),
  and meter events, kept by identifier the way Stripe dedups them.
  Every request is kept, with its form decoded, so a test can see what
  the relay asked. stripe-mock checks the shapes (stripe_mock_test.exs);
  this checks the flows.
  """

  defmodule Plug do
    @moduledoc false
    @behaviour Elixir.Plug
    import Elixir.Plug.Conn

    @impl true
    def init(agent), do: agent

    @impl true
    def call(conn, agent) do
      {:ok, body, conn} = read_body(conn)

      params =
        Elixir.Plug.Conn.Query.decode(
          body <> if(conn.query_string != "", do: "&" <> conn.query_string, else: "")
        )

      req = %{
        method: conn.method,
        path: conn.request_path,
        params: params,
        auth: get_req_header(conn, "authorization") |> List.first(),
        idempotency_key: get_req_header(conn, "idempotency-key") |> List.first(),
        version: get_req_header(conn, "stripe-version") |> List.first()
      }

      {status, reply} =
        Agent.get_and_update(agent, fn s ->
          s = %{s | requests: s.requests ++ [req]}

          case s.fail do
            [{path, status} | rest] when path == req.path ->
              {{status, %{error: %{message: "a failure the test asked for"}}}, %{s | fail: rest}}

            _ ->
              route(req, s)
          end
        end)

      conn
      |> put_resp_content_type("application/json")
      |> send_resp(status, Jason.encode!(reply))
    end

    defp route(%{method: "POST", path: "/v1/customers"} = r, s) do
      id = "cus_fake#{map_size(s.customers) + 1}"

      {{200, %{id: id, object: "customer", email: r.params["email"]}},
       put_in(s.customers[id], r.params)}
    end

    defp route(%{method: "POST", path: "/v1/checkout/sessions"}, s) do
      id = "cs_fake#{length(s.requests)}"
      {{200, %{id: id, object: "checkout.session", url: "https://checkout.stripe.test/#{id}"}}, s}
    end

    defp route(%{method: "POST", path: "/v1/billing_portal/sessions"} = r, s) do
      {{200, %{id: "bps_fake", url: "https://billing.stripe.test/p/#{r.params["customer"]}"}}, s}
    end

    defp route(%{method: "GET", path: "/v1/subscriptions/" <> id}, s) do
      case s.subscriptions[id] do
        nil -> {{404, %{error: %{message: "No such subscription: '#{id}'"}}}, s}
        sub -> {{200, sub}, s}
      end
    end

    defp route(%{method: "POST", path: "/v1/billing/meter_events"} = r, s) do
      id = r.params["identifier"]

      if Map.has_key?(s.meter_events, id) do
        {{400, %{error: %{message: "An event with identifier #{id} already exists."}}}, s}
      else
        event = %{
          identifier: id,
          event_name: r.params["event_name"],
          customer: get_in(r.params, ["payload", "stripe_customer_id"]),
          value: String.to_integer(get_in(r.params, ["payload", "value"])),
          timestamp: String.to_integer(r.params["timestamp"])
        }

        {{200, %{object: "billing.meter_event", identifier: id}},
         %{s | meter_events: Map.put(s.meter_events, id, event), order: s.order ++ [id]}}
      end
    end

    defp route(_, s), do: {{404, %{error: %{message: "the fake has no such route"}}}, s}
  end

  @doc "Starts the fake (under the test's supervisor): `%{url, agent}`."
  def start do
    {:ok, agent} =
      Agent.start_link(fn ->
        %{
          requests: [],
          customers: %{},
          subscriptions: %{},
          meter_events: %{},
          order: [],
          fail: []
        }
      end)

    pid =
      ExUnit.Callbacks.start_supervised!(
        {Bandit, plug: {Plug, agent}, ip: :loopback, port: 0, startup_log: false},
        id: make_ref()
      )

    {:ok, {_, port}} = ThousandIsland.listener_info(pid)
    %{url: "http://127.0.0.1:#{port}", agent: agent}
  end

  @doc "A Stripe config for the relay, pointing at `fake` (billing on)."
  def config(%{url: url}) do
    %{
      secret_key: "sk_test_fake",
      webhook_secret: "whsec_test_fake",
      prices: %{base: "price_base", mau: "price_mau", minutes: "price_minutes"},
      api_base: url,
      live: false
    }
  end

  @doc "Stripe now has subscription `sub` (a map with string keys, as Stripe sends)."
  def put_subscription(%{agent: a}, %{"id" => id} = sub),
    do: Agent.update(a, &put_in(&1.subscriptions[id], sub))

  @doc "The next request to `path` fails with `status`."
  def fail_next(%{agent: a}, path, status),
    do: Agent.update(a, &%{&1 | fail: &1.fail ++ [{path, status}]})

  def requests(%{agent: a}), do: Agent.get(a, & &1.requests)
  def requests(fake, path), do: fake |> requests() |> Enum.filter(&(&1.path == path))

  @doc "The meter events Stripe kept, in the order it took them."
  def meter_events(%{agent: a}),
    do: Agent.get(a, fn s -> Enum.map(s.order, &s.meter_events[&1]) end)

  @doc "What a meter's events sum to for a customer: what Stripe would bill."
  def total(fake, event_name, customer) do
    fake
    |> meter_events()
    |> Enum.filter(&(&1.event_name == event_name and &1.customer == customer))
    |> Enum.map(& &1.value)
    |> Enum.sum()
  end
end
