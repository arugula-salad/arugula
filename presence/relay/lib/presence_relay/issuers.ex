defmodule PresenceRelay.Issuers do
  @moduledoc """
  Frontend-only auth (#318 P4): an app names its sign-in provider, and a
  page with the app's publishable key joins a room with the end user's own
  token from that provider (`auth`), which the relay checks against the
  provider's published keys. The app needs no server of its own.

  One issuer per app: a preset and its settings.

  | provider | settings | `iss` | keys |
  |---|---|---|---|
  | `clerk` | `issuer`: the Frontend API URL | that URL | `<iss>/.well-known/jwks.json` |
  | `auth0` | `issuer`: the tenant's domain (or custom domain) | `https://<domain>/` | `<iss>.well-known/jwks.json` |
  | `firebase` | `project`: the Firebase project id | `https://securetoken.google.com/<project>` | Google's securetoken JWKS |
  | `supabase` | `issuer`: the project URL | `<url>/auth/v1` | `<iss>/.well-known/jwks.json` |
  | `oidc` | `issuer`: the issuer URL, exactly as its tokens' `iss` | that URL | its discovery document's `jwks_uri` |

  `audience` is the `aud` a token must carry: required for Auth0 (the API
  identifier, or the client id for ID tokens) and OIDC; Firebase's is the
  project id; Supabase's defaults to `authenticated`; Clerk's session
  tokens carry none (checked only when set). `name_claim` is where the
  member's name is (`user_metadata.full_name` walks into maps): `name` by
  default, Supabase's `user_metadata.full_name`.

  A **room rule** (optional) limits a member's rooms by a claim:

  - `prefix`: the claim (a string) must equal the room's id before its
    first `:`. `{"claim": "org_id", "match": "prefix"}` lets a member of
    org `acme` into `acme:standup` and no other org's rooms.
  - `contains`: the claim must be a list holding the room's id (or a
    string equal to it).

  Nothing else is a rule. Guests (where the app allows them) come by name
  and aren't held to it.

  Checks (`verify/3`): RS256, ES256 or EdDSA only, with the provider's key
  for the token's `kid` (a kid it lacks fetches the keys once more,
  rate-limited: `PresenceRelay.Issuers.Keys`); `iss` exactly; `aud`;
  `exp` and `nbf` with 30 seconds' grace; a `sub` of 1–128
  printable characters; and the preset's own: Clerk's `azp` (when there)
  must be the page's origin and a `pending` session is refused; Firebase's
  `auth_time` mustn't be in the future. A legacy Supabase project signing
  with its HS256 secret isn't supported (its JWKS is empty): it needs
  asymmetric signing keys.

  A member's `sub` is `<provider>-<tag>:<their sub>`, the tag the first 8
  hex digits of SHA-256 of the `iss`, so two issuers' users never meet
  as one, and none looks like a guest (`g_…`).
  """

  import Ecto.Query, only: [from: 2]
  alias PresenceRelay.Repo
  alias PresenceRelay.Apps.App
  alias PresenceRelay.Issuers.{Issuer, JWT, Keys}

  @providers ~w(clerk auth0 firebase supabase oidc)
  @matches ~w(prefix contains)
  @firebase_jwks "https://www.googleapis.com/service_accounts/v1/jwk/securetoken@system.gserviceaccount.com"
  # A provider's clock a little off ours isn't a forged token.
  @leeway 30
  @claim ~r/\A[\x21-\x7e]{1,128}\z/

  def providers, do: @providers

  ## The app's issuer

  @doc "`app`'s issuer, or nil."
  def get(%App{id: id}), do: Repo.get(Issuer, id)

  @doc "Sets `app`'s issuer from `params` (the API's JSON): `{:ok, issuer}` or `{:error, message}`."
  def put(%App{id: id}, params) do
    with {:ok, attrs} <- cast(params) do
      Repo.insert(struct(Issuer, Map.merge(attrs, %{app_id: id})),
        on_conflict: {:replace, Map.keys(attrs) ++ [:updated_at]},
        conflict_target: :app_id,
        returning: true
      )
      |> case do
        {:ok, i} -> {:ok, i}
        {:error, _} -> {:error, "couldn't save the issuer"}
      end
    end
  end

  @doc "Takes `app`'s issuer away: its members' tokens stop working."
  def delete(%App{id: id}) do
    Repo.delete_all(from(i in Issuer, where: i.app_id == ^id))
    :ok
  end

  @doc "What the API shows: the settings, and the `iss` and key URLs they make."
  def view(nil), do: nil

  def view(%Issuer{} = i) do
    e = endpoints(i)

    %{
      provider: i.provider,
      issuer: i.issuer,
      project: i.project,
      audience: i.audience,
      name_claim: i.name_claim,
      room_rule: i.room_claim && %{claim: i.room_claim, match: i.room_match},
      iss: e.iss,
      jwks: e[:jwks],
      discovery: e[:discovery]
    }
  end

  @doc """
  An issuer's settings from JSON, checked, with the preset's defaults:
  `{:ok, attrs}` for `Issuer`, or `{:error, message}`.
  """
  def cast(params) when is_map(params) do
    p = fn k -> params[k] |> then(&if(is_binary(&1), do: String.trim(&1), else: &1)) end
    provider = p.("provider")

    with true <-
           provider in @providers ||
             {:error, "a provider is one of #{Enum.join(@providers, ", ")}"},
         {:ok, issuer, project} <- place(provider, p.("issuer"), p.("project")),
         {:ok, audience} <- audience(provider, p.("audience"), project),
         {:ok, name_claim} <- claim(p.("name_claim") || default_name(provider), "name_claim"),
         {:ok, {room_claim, room_match}} <- room_rule(params["room_rule"]) do
      {:ok,
       %{
         provider: provider,
         issuer: issuer,
         project: project,
         audience: audience,
         name_claim: name_claim,
         room_claim: room_claim,
         room_match: room_match
       }}
    end
  end

  def cast(_), do: {:error, "an issuer is a JSON object"}

  defp place("firebase", _, project) do
    if is_binary(project) and project =~ ~r/\A[a-z][a-z0-9-]{3,29}\z/,
      do: {:ok, nil, project},
      else: {:error, "Firebase needs its project id (project)"}
  end

  defp place(provider, issuer, _) do
    with {:ok, u} <- url(issuer) do
      bare = u.path in [nil, "", "/"]

      case provider do
        "clerk" when bare ->
          {:ok, origin(u), nil}

        "auth0" when bare ->
          {:ok, origin(u) <> "/", nil}

        "supabase" when bare or u.path in ["/auth/v1", "/auth/v1/"] ->
          {:ok, origin(u), nil}

        "oidc" ->
          {:ok, issuer, nil}

        _ ->
          {:error, "the issuer for #{provider} is just https://host (no path)"}
      end
    end
  end

  # An Auth0 domain may come bare (acme.us.auth0.com).
  defp url(s) when is_binary(s) and s != "" do
    s = if s =~ ~r{\A[a-z][a-z0-9+.-]*://}i, do: s, else: "https://" <> s
    u = URI.parse(s)
    local? = Application.get_env(:presence_relay, :issuer_allow_local, false)

    cond do
      String.length(s) > 255 ->
        {:error, "the issuer URL is too long"}

      u.scheme != "https" and not (local? and u.scheme == "http") ->
        {:error, "the issuer is https://"}

      not is_binary(u.host) or u.host == "" ->
        {:error, "the issuer needs a host"}

      not local? and not Keys.public_name?(String.downcase(u.host)) ->
        {:error, "the issuer's host isn't public"}

      u.query != nil or u.fragment != nil or u.userinfo != nil ->
        {:error, "the issuer has no query or user"}

      true ->
        {:ok, u}
    end
  end

  defp url(_), do: {:error, "this provider needs its URL (issuer)"}

  defp origin(%URI{scheme: s, host: h, port: port}) do
    default = if s == "https", do: 443, else: 80

    if port && port != default,
      do: "#{s}://#{String.downcase(h)}:#{port}",
      else: "#{s}://#{String.downcase(h)}"
  end

  defp audience("firebase", _, project), do: {:ok, project}
  defp audience("supabase", nil, _), do: {:ok, "authenticated"}
  defp audience(p, nil, _) when p in ~w(auth0 oidc), do: {:error, "#{p} needs an audience"}
  defp audience(_, nil, _), do: {:ok, nil}

  defp audience(_, a, _) when is_binary(a) and a != "" and byte_size(a) <= 255, do: {:ok, a}
  defp audience(_, _, _), do: {:error, "an audience is a string of up to 255 bytes"}

  defp default_name("supabase"), do: "user_metadata.full_name"
  defp default_name(_), do: "name"

  defp claim(c, what) do
    if is_binary(c) and c =~ @claim,
      do: {:ok, c},
      else: {:error, "#{what} is a claim name (a.b walks into maps), up to 128 characters"}
  end

  defp room_rule(nil), do: {:ok, {nil, nil}}

  defp room_rule(%{"claim" => c, "match" => m} = rule) when map_size(rule) == 2 do
    with {:ok, c} <- claim(c, "a room rule's claim"),
         true <- m in @matches || {:error, "a room rule's match is prefix or contains"},
         do: {:ok, {c, m}}
  end

  defp room_rule(_), do: {:error, ~s(a room rule is {"claim": …, "match": "prefix" | "contains"})}

  @doc "The `iss` an issuer's tokens carry, and where its keys are (`jwks`, or `discovery` to find them)."
  def endpoints(%Issuer{provider: "clerk", issuer: i}),
    do: %{iss: i, jwks: i <> "/.well-known/jwks.json"}

  def endpoints(%Issuer{provider: "auth0", issuer: i}),
    do: %{iss: i, jwks: i <> ".well-known/jwks.json"}

  def endpoints(%Issuer{provider: "firebase", project: p}),
    do: %{
      iss: "https://securetoken.google.com/" <> p,
      jwks: Application.get_env(:presence_relay, :firebase_jwks, @firebase_jwks)
    }

  def endpoints(%Issuer{provider: "supabase", issuer: i}),
    do: %{iss: i <> "/auth/v1", jwks: i <> "/auth/v1/.well-known/jwks.json"}

  def endpoints(%Issuer{provider: "oidc", issuer: i}),
    do: %{iss: i, discovery: String.trim_trailing(i, "/") <> "/.well-known/openid-configuration"}

  ## Verifying a member's token

  @doc """
  Checks `token` against `issuer` for a socket: `{:ok, %{sub, name}}` or
  `{:error, reason}`. Options: `origin` (the page's), `room` (for the
  room rule), `name` (the page's name for them, used only when the token
  has none), `now` (Unix seconds, tests).

  The cheap checks (algorithm, `iss`, expiry) come before any key is
  fetched, so a token that can't be good costs no request.
  """
  def verify(%Issuer{} = i, token, opts \\ []) do
    now = opts[:now] || System.system_time(:second)
    e = endpoints(i)

    with {:ok, header, claims, input, sig} <- JWT.parse(token),
         true <- JWT.alg_ok?(header) || {:error, :bad_alg},
         true <- claims["iss"] == e.iss || {:error, :wrong_issuer},
         :ok <- times(claims, now),
         {:ok, jwk} <- key(e, header),
         true <- JWT.verify(header["alg"], jwk, input, sig) || {:error, :bad_signature},
         :ok <- audience_ok(i.audience, claims["aud"]),
         :ok <- preset(i.provider, claims, now, opts),
         {:ok, sub} <- sub(claims["sub"]),
         :ok <- room_ok(i, claims, opts[:room]) do
      {:ok, %{sub: scoped(i.provider, e.iss, sub), name: name(i, claims, opts[:name])}}
    end
  end

  defp times(c, now) do
    cond do
      not is_number(c["exp"]) -> {:error, :no_expiry}
      c["exp"] + @leeway <= now -> {:error, :expired}
      Map.has_key?(c, "nbf") and not is_number(c["nbf"]) -> {:error, :bad_nbf}
      is_number(c["nbf"]) and c["nbf"] - @leeway > now -> {:error, :not_yet}
      true -> :ok
    end
  end

  defp key(e, header) do
    with {:ok, url} <- jwks_url(e),
         {:ok, jwks} <- Keys.get(url) do
      case JWT.key(jwks, header) do
        nil ->
          # Maybe a key the provider has just rotated in: ask again (or,
          # if it was asked lately, not).
          with {:ok, jwks} <- Keys.refresh(url),
               %{} = k <- JWT.key(jwks, header),
               do: {:ok, k},
               else: (_ -> {:error, :unknown_key})

        k ->
          {:ok, k}
      end
    end
  end

  defp jwks_url(%{jwks: url}), do: {:ok, url}

  # OIDC discovery: the document must name the configured issuer exactly.
  defp jwks_url(%{discovery: url, iss: iss}) do
    case Keys.get(url) do
      {:ok, %{"issuer" => ^iss, "jwks_uri" => jwks}} when is_binary(jwks) -> {:ok, jwks}
      {:ok, _} -> {:error, :discovery_mismatch}
      error -> error
    end
  end

  defp audience_ok(nil, _), do: :ok
  defp audience_ok(a, a), do: :ok

  defp audience_ok(a, list) when is_list(list),
    do: if(a in list, do: :ok, else: {:error, :wrong_audience})

  defp audience_ok(_, _), do: {:error, :wrong_audience}

  # Clerk: `azp` is the origin the session token was made for.
  defp preset("clerk", c, _, opts) do
    cond do
      c["sts"] == "pending" -> {:error, :pending_session}
      is_binary(c["azp"]) and c["azp"] != opts[:origin] -> {:error, :wrong_party}
      true -> :ok
    end
  end

  defp preset("firebase", c, now, _) do
    if is_number(c["auth_time"]) and c["auth_time"] - @leeway <= now,
      do: :ok,
      else: {:error, :bad_auth_time}
  end

  defp preset(_, _, _, _), do: :ok

  defp sub(s) do
    if is_binary(s) and s =~ ~r/\A[\x21-\x7e]{1,128}\z/, do: {:ok, s}, else: {:error, :bad_sub}
  end

  defp scoped(provider, iss, sub) do
    tag = :crypto.hash(:sha256, iss) |> binary_part(0, 4) |> Base.encode16(case: :lower)
    "#{provider}-#{tag}:#{sub}"
  end

  defp room_ok(%Issuer{room_claim: nil}, _, _), do: :ok

  defp room_ok(%Issuer{room_claim: c, room_match: "prefix"}, claims, room) do
    case {claim_at(claims, c), String.split(room || "", ":", parts: 2)} do
      {v, [v, _]} when is_binary(v) and v != "" -> :ok
      _ -> {:error, :room_rule}
    end
  end

  defp room_ok(%Issuer{room_claim: c, room_match: "contains"}, claims, room) do
    case claim_at(claims, c) do
      list when is_list(list) -> if(room in list, do: :ok, else: {:error, :room_rule})
      ^room when is_binary(room) -> :ok
      _ -> {:error, :room_rule}
    end
  end

  defp room_ok(_, _, _), do: {:error, :room_rule}

  @doc "A claim by name, or (when there's no claim by that whole name) by a dotted path into maps."
  def claim_at(claims, path) do
    case Map.fetch(claims, path) do
      {:ok, v} ->
        v

      :error ->
        Enum.reduce_while(String.split(path, "."), claims, fn k, acc ->
          if is_map(acc) and Map.has_key?(acc, k), do: {:cont, acc[k]}, else: {:halt, nil}
        end)
    end
  end

  # The token's name; else the page's; else a placeholder.
  defp name(i, claims, fallback) do
    [claim_at(claims, i.name_claim), fallback]
    |> Enum.map(&(is_binary(&1) && String.trim(&1)))
    |> Enum.find(&(is_binary(&1) and &1 != ""))
    |> case do
      nil -> "Member"
      n -> String.slice(n, 0, 80)
    end
  end
end
