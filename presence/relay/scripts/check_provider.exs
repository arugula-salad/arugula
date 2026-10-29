# Would the relay admit this sign-in token? (#318 P4)
#
# Checks a real provider's token against an issuer configured as an app
# would configure it, exactly as the socket does (PresenceRelay.Issuers),
# with no database and no relay running. The token is read from stdin or
# AUTH_TOKEN, never argv (so it stays out of shell history and `ps`).
#
#   cd presence/relay
#   pbpaste | mise exec -- mix run --no-start scripts/check_provider.exs -- \
#     --provider clerk --issuer https://your-app.clerk.accounts.dev \
#     --origin http://localhost:3000
#
#   AUTH_TOKEN=… mise exec -- mix run --no-start scripts/check_provider.exs -- \
#     --provider firebase --project my-project
#
# Options: --provider (clerk|auth0|firebase|supabase|oidc), --issuer,
# --project, --audience, --name-claim, --room-claim and --room-match (a
# room rule), --room (default lobby), --origin (the page's, for Clerk's
# azp), --allow-local (http and local hosts, to try it on a test issuer).
# Prints the token's header and claims it checked (not the token), then
# "admitted" with the member's sub and name, or "refused" and why; exits
# 0 or 1.

{opts, _, bad} =
  OptionParser.parse(System.argv(),
    strict: [
      provider: :string,
      issuer: :string,
      project: :string,
      audience: :string,
      name_claim: :string,
      room_claim: :string,
      room_match: :string,
      room: :string,
      origin: :string,
      allow_local: :boolean
    ]
  )

fail = fn msg ->
  IO.puts(:stderr, msg)
  System.halt(2)
end

if bad != [], do: fail.("unknown options: #{inspect(bad)}")
if !opts[:provider], do: fail.("--provider is one of #{Enum.join(PresenceRelay.Issuers.providers(), ", ")}")

if opts[:allow_local], do: Application.put_env(:presence_relay, :issuer_allow_local, true)

# What the socket needs and nothing more: HTTP and the key cache.
{:ok, _} = Application.ensure_all_started([:crypto, :ssl, :jason, :finch])
{:ok, _} = Finch.start_link(name: PresenceRelay.Finch)
{:ok, _} = PresenceRelay.Issuers.Keys.start_link([])

token =
  case System.get_env("AUTH_TOKEN") do
    t when is_binary(t) and t != "" -> t
    _ -> IO.read(:stdio, :eof) |> to_string()
  end
  |> String.trim()

if token == "", do: fail.("no token: pipe it in, or set AUTH_TOKEN")

params =
  %{
    "provider" => opts[:provider],
    "issuer" => opts[:issuer],
    "project" => opts[:project],
    "audience" => opts[:audience],
    "name_claim" => opts[:name_claim],
    "room_rule" =>
      opts[:room_claim] && %{"claim" => opts[:room_claim], "match" => opts[:room_match]}
  }
  |> Map.reject(fn {_, v} -> is_nil(v) end)

issuer =
  case PresenceRelay.Issuers.cast(params) do
    {:ok, attrs} -> struct(PresenceRelay.Issuers.Issuer, attrs)
    {:error, msg} -> fail.("the issuer settings: #{msg}")
  end

IO.puts("issuer: #{inspect(PresenceRelay.Issuers.view(issuer), pretty: true)}")

case PresenceRelay.Issuers.JWT.parse(token) do
  {:ok, header, claims, _, _} ->
    now = System.system_time(:second)
    shown = Map.take(claims, ~w(iss aud sub azp exp nbf auth_time org_id o sts))
    IO.puts("token header: #{inspect(Map.take(header, ~w(alg kid typ)))}")
    IO.puts("token claims: #{inspect(shown)}")
    if is_number(claims["exp"]), do: IO.puts("expires in: #{round(claims["exp"] - now)} s")

  {:error, _} ->
    IO.puts("token: not a JWS (header.claims.signature)")
end

room = opts[:room] || "lobby"

case PresenceRelay.Issuers.verify(issuer, token, origin: opts[:origin], room: room) do
  {:ok, m} ->
    IO.puts("admitted to #{room}: sub #{m.sub}, name #{inspect(m.name)}")

  {:error, reason} ->
    IO.puts("refused: #{inspect(reason)}")
    System.halt(1)
end
