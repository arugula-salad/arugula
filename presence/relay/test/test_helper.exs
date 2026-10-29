# stripe-mock's tests (stripe_mock_test.exs) run where STRIPE_MOCK_URL
# points at one (CI has it as a service).
ExUnit.start(exclude: if(System.get_env("STRIPE_MOCK_URL"), do: [], else: [:stripe_mock]))
Ecto.Adapters.SQL.Sandbox.mode(PresenceRelay.Repo, :manual)
