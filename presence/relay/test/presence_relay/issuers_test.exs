defmodule PresenceRelay.IssuersTest do
  # The JWKS cache is the app's, one per node: these run one at a time,
  # each against an issuer (and so URLs) of its own.
  use ExUnit.Case, async: false

  alias PresenceRelay.Issuers
  alias PresenceRelay.Issuers.{Issuer, Keys}
  alias PresenceRelay.TestIssuer, as: T

  setup_all do
    %{rsa: T.rsa("rsa-1"), rsa2: T.rsa("rsa-2"), ec: T.ec("ec-1"), ed: T.ed("ed-1")}
  end

  setup ctx do
    srv = T.start([ctx.rsa, ctx.ec, ctx.ed])
    issuer = %Issuer{provider: "oidc", issuer: srv.url, audience: "my-app", name_claim: "name"}
    %{srv: srv, issuer: issuer}
  end

  defp claims(srv, more \\ %{}),
    do:
      Map.merge(%{"iss" => srv.url, "aud" => "my-app", "sub" => "user_1", "name" => "Ada"}, more)

  defp now, do: System.system_time(:second)

  test "an OIDC issuer's RS256, ES256 and EdDSA tokens are admitted, by discovery, keys cached",
       ctx do
    for key <- [ctx.rsa, ctx.ec, ctx.ed] do
      assert {:ok, %{sub: "oidc-" <> _ = sub, name: "Ada"}} =
               Issuers.verify(ctx.issuer, T.sign(key, claims(ctx.srv)))

      assert [_, "user_1"] = String.split(sub, ":", parts: 2)
    end

    # One discovery and one JWKS fetch for all three.
    assert T.hits(ctx.srv, "/.well-known/openid-configuration") == 1
    assert T.jwks_hits(ctx.srv) == 1
  end

  test "wrong iss, aud, expired, not yet, and a missing exp are refused", ctx do
    s = ctx.srv
    v = &Issuers.verify(ctx.issuer, T.sign(ctx.rsa, &1))

    assert {:error, :wrong_issuer} = v.(claims(s, %{"iss" => "https://evil.example"}))
    assert {:error, :wrong_audience} = v.(claims(s, %{"aud" => "someone-else"}))
    assert {:error, :wrong_audience} = v.(Map.delete(claims(s), "aud"))
    assert {:ok, _} = v.(claims(s, %{"aud" => ["other", "my-app"]}))
    assert {:error, :expired} = v.(claims(s, %{"exp" => now() - 120}))
    # A little leeway either way.
    assert {:ok, _} = v.(claims(s, %{"exp" => now() - 5}))
    assert {:error, :not_yet} = v.(claims(s, %{"nbf" => now() + 600}))
    assert {:ok, _} = v.(claims(s, %{"nbf" => now() + 5}))

    assert {:error, :no_expiry} =
             Issuers.verify(ctx.issuer, T.sign(ctx.rsa, claims(s, %{"exp" => nil})))

    assert {:error, :bad_sub} = v.(claims(s, %{"sub" => ""}))
  end

  test "none, HS256, a forged signature and a key of another type are refused", ctx do
    c = claims(ctx.srv)
    assert {:error, :bad_alg} = Issuers.verify(ctx.issuer, T.sign(ctx.rsa, c, %{"alg" => "none"}))

    assert {:error, :bad_alg} =
             Issuers.verify(ctx.issuer, T.sign(ctx.rsa, c, %{"alg" => "HS256"}))

    assert {:error, :bad_alg} =
             Issuers.verify(ctx.issuer, T.sign(ctx.rsa, c, %{"alg" => "RS512"}))

    # Signed by a key the issuer doesn't have, under its key's id.
    assert {:error, :bad_signature} =
             Issuers.verify(ctx.issuer, T.sign(ctx.rsa2, c, %{"kid" => "rsa-1"}))

    # An ES256 token naming the RSA key's id: no key of that type has it.
    assert {:error, :unknown_key} =
             Issuers.verify(ctx.issuer, T.sign(ctx.ec, c, %{"kid" => "rsa-1"}))

    assert {:error, :malformed} = Issuers.verify(ctx.issuer, "not.a-token")
  end

  test "a rotated-in key is fetched once more; then a stream of unknown kids doesn't refetch",
       ctx do
    assert {:ok, _} = Issuers.verify(ctx.issuer, T.sign(ctx.rsa, claims(ctx.srv)))
    assert T.jwks_hits(ctx.srv) == 1

    # The provider rotates: rsa-2 in, rsa-1 out.
    T.set_keys(ctx.srv, [ctx.rsa2])
    Process.sleep(350)
    assert {:ok, _} = Issuers.verify(ctx.issuer, T.sign(ctx.rsa2, claims(ctx.srv)))
    assert T.jwks_hits(ctx.srv) == 2

    Process.sleep(350)
    # The first unknown kid may ask again; the rest, inside the window, don't.
    for n <- 1..20 do
      assert {:error, :unknown_key} =
               Issuers.verify(
                 ctx.issuer,
                 T.sign(ctx.rsa, claims(ctx.srv), %{"kid" => "made-up-#{n}"})
               )
    end

    assert T.jwks_hits(ctx.srv) == 3

    # Concurrently, too: one fetch at most.
    Process.sleep(350)

    1..20
    |> Enum.map(fn n ->
      Task.async(fn ->
        Issuers.verify(ctx.issuer, T.sign(ctx.rsa, claims(ctx.srv), %{"kid" => "k#{n}"}))
      end)
    end)
    |> Enum.each(&assert({:error, :unknown_key} = Task.await(&1)))

    assert T.jwks_hits(ctx.srv) == 4
  end

  test "max-age is honoured within bounds: max-age=0 still caches for a minute", ctx do
    T.set(ctx.srv, :cache, "public, max-age=0")
    assert {:ok, _} = Issuers.verify(ctx.issuer, T.sign(ctx.rsa, claims(ctx.srv)))
    assert {:ok, _} = Issuers.verify(ctx.issuer, T.sign(ctx.ec, claims(ctx.srv)))
    assert T.jwks_hits(ctx.srv) == 1
  end

  test "a discovery document naming another issuer is refused", ctx do
    srv = T.start([ctx.rsa], issuer: "https://someone-else.example")
    i = %{ctx.issuer | issuer: srv.url}
    assert {:error, :discovery_mismatch} = Issuers.verify(i, T.sign(ctx.rsa, claims(srv)))
    assert T.jwks_hits(srv) == 0
  end

  test "an unreachable issuer is refused, and not asked again at once", ctx do
    i = %{ctx.issuer | issuer: "http://127.0.0.1:1"}
    tok = T.sign(ctx.rsa, claims(ctx.srv, %{"iss" => "http://127.0.0.1:1"}))
    assert {:error, {:http, _}} = Issuers.verify(i, tok)
    assert {:error, {:http, _}} = Issuers.verify(i, tok)
  end

  test "a slow issuer times out", ctx do
    T.set(ctx.srv, :delay, 1_500)
    assert {:error, {:http, _}} = Issuers.verify(ctx.issuer, T.sign(ctx.rsa, claims(ctx.srv)))
  end

  test "room rules: prefix and contains, by a claim (dotted walks into maps)", ctx do
    prefix = %{ctx.issuer | room_claim: "org_id", room_match: "prefix"}
    tok = T.sign(ctx.rsa, claims(ctx.srv, %{"org_id" => "acme"}))
    assert {:ok, _} = Issuers.verify(prefix, tok, room: "acme:standup")
    assert {:error, :room_rule} = Issuers.verify(prefix, tok, room: "globex:standup")
    assert {:error, :room_rule} = Issuers.verify(prefix, tok, room: "acme")

    assert {:error, :room_rule} =
             Issuers.verify(prefix, T.sign(ctx.rsa, claims(ctx.srv)), room: "acme:x")

    # Clerk's v2 session tokens keep the org under `o`.
    clerkish = %{prefix | room_claim: "o.id"}
    tok = T.sign(ctx.rsa, claims(ctx.srv, %{"o" => %{"id" => "org_2x"}}))
    assert {:ok, _} = Issuers.verify(clerkish, tok, room: "org_2x:lobby")

    contains = %{ctx.issuer | room_claim: "rooms", room_match: "contains"}
    tok = T.sign(ctx.rsa, claims(ctx.srv, %{"rooms" => ["design", "standup"]}))
    assert {:ok, _} = Issuers.verify(contains, tok, room: "design")
    assert {:error, :room_rule} = Issuers.verify(contains, tok, room: "payroll")
  end

  test "the name: the claim, else the page's, else a placeholder; long ones cut", ctx do
    nameless = Map.delete(claims(ctx.srv), "name")

    assert {:ok, %{name: "Ada L"}} =
             Issuers.verify(ctx.issuer, T.sign(ctx.rsa, nameless), name: "Ada L")

    assert {:ok, %{name: "Member"}} = Issuers.verify(ctx.issuer, T.sign(ctx.rsa, nameless))

    assert {:ok, %{name: "Ada"}} =
             Issuers.verify(ctx.issuer, T.sign(ctx.rsa, claims(ctx.srv)), name: "Someone else")

    long = T.sign(ctx.rsa, claims(ctx.srv, %{"name" => String.duplicate("a", 200)}))
    assert {:ok, %{name: n}} = Issuers.verify(ctx.issuer, long)
    assert String.length(n) == 80
  end

  test "two issuers' users never share a sub", ctx do
    other = T.start([ctx.rsa])
    i2 = %{ctx.issuer | issuer: other.url}
    {:ok, %{sub: a}} = Issuers.verify(ctx.issuer, T.sign(ctx.rsa, claims(ctx.srv)))
    {:ok, %{sub: b}} = Issuers.verify(i2, T.sign(ctx.rsa, claims(other)))
    assert a != b
  end

  describe "presets" do
    test "Firebase: securetoken's iss for the project, aud the project id, auth_time in the past",
         ctx do
      Application.put_env(:presence_relay, :firebase_jwks, ctx.srv.url <> "/firebase/jwks.json")
      on_exit(fn -> Application.delete_env(:presence_relay, :firebase_jwks) end)
      {:ok, attrs} = Issuers.cast(%{"provider" => "firebase", "project" => "my-proj"})
      i = struct(Issuer, attrs)
      assert i.audience == "my-proj"

      c = %{
        "iss" => "https://securetoken.google.com/my-proj",
        "aud" => "my-proj",
        "sub" => "Uid28chars",
        "name" => "Ada",
        "auth_time" => now() - 60
      }

      assert {:ok, %{sub: "firebase-" <> _}} = Issuers.verify(i, T.sign(ctx.rsa, c))

      assert {:error, :wrong_audience} =
               Issuers.verify(i, T.sign(ctx.rsa, %{c | "aud" => "other-proj"}))

      assert {:error, :wrong_issuer} =
               Issuers.verify(
                 i,
                 T.sign(ctx.rsa, %{c | "iss" => "https://securetoken.google.com/other"})
               )

      assert {:error, :bad_auth_time} =
               Issuers.verify(i, T.sign(ctx.rsa, %{c | "auth_time" => now() + 3600}))
    end

    test "Supabase: <project>/auth/v1, aud authenticated, the name in user_metadata, ES256",
         ctx do
      {:ok, attrs} = Issuers.cast(%{"provider" => "supabase", "issuer" => ctx.srv.url <> "/"})
      i = struct(Issuer, attrs)
      assert Issuers.endpoints(i).jwks == ctx.srv.url <> "/auth/v1/.well-known/jwks.json"

      c = %{
        "iss" => ctx.srv.url <> "/auth/v1",
        "aud" => "authenticated",
        "sub" => "6a1c0e2e-1f7a-4b8e-9d0a-1c2b3d4e5f60",
        "role" => "authenticated",
        "user_metadata" => %{"full_name" => "Ada Lovelace"}
      }

      assert {:ok, %{name: "Ada Lovelace"}} = Issuers.verify(i, T.sign(ctx.ec, c))
      assert T.hits(ctx.srv, "/auth/v1/.well-known/jwks.json") == 1
      assert {:error, :wrong_audience} = Issuers.verify(i, T.sign(ctx.ec, %{c | "aud" => "anon"}))
    end

    test "Clerk: no aud needed, azp must be the page's origin, pending sessions refused", ctx do
      {:ok, attrs} = Issuers.cast(%{"provider" => "clerk", "issuer" => ctx.srv.url})
      i = struct(Issuer, attrs)

      c = %{
        "iss" => ctx.srv.url,
        "sub" => "user_2abc",
        "azp" => "https://app.acme.com",
        "sid" => "sess_1"
      }

      assert {:ok, %{sub: "clerk-" <> _, name: "Member"}} =
               Issuers.verify(i, T.sign(ctx.rsa, c), origin: "https://app.acme.com")

      assert {:error, :wrong_party} =
               Issuers.verify(i, T.sign(ctx.rsa, c), origin: "https://evil.com")

      assert {:ok, _} =
               Issuers.verify(i, T.sign(ctx.rsa, Map.delete(c, "azp")), origin: "https://x.com")

      assert {:error, :pending_session} =
               Issuers.verify(i, T.sign(ctx.rsa, Map.put(c, "sts", "pending")),
                 origin: "https://app.acme.com"
               )

      assert T.hits(ctx.srv, "/.well-known/jwks.json") == 1
    end

    test "Auth0: a bare domain becomes https://<domain>/, the audience required", ctx do
      assert {:error, "auth0 needs an audience"} =
               Issuers.cast(%{"provider" => "auth0", "issuer" => "acme.us.auth0.com"})

      {:ok, attrs} =
        Issuers.cast(%{
          "provider" => "auth0",
          "issuer" => "acme.us.auth0.com",
          "audience" => "https://api.acme.com"
        })

      assert attrs.issuer == "https://acme.us.auth0.com/"

      # Against the local issuer: iss with its trailing slash.
      {:ok, attrs} =
        Issuers.cast(%{"provider" => "auth0", "issuer" => ctx.srv.url, "audience" => "api"})

      i = struct(Issuer, attrs)

      c = %{
        "iss" => ctx.srv.url <> "/",
        "aud" => ["api", ctx.srv.url <> "/userinfo"],
        "sub" => "google-oauth2|1234"
      }

      assert {:ok, %{sub: sub}} = Issuers.verify(i, T.sign(ctx.rsa, c))
      assert String.ends_with?(sub, ":google-oauth2|1234")

      assert {:error, :wrong_issuer} =
               Issuers.verify(i, T.sign(ctx.rsa, %{c | "iss" => ctx.srv.url}))
    end
  end

  describe "settings" do
    test "checked, with each preset's defaults" do
      assert {:error, "a provider is" <> _} = Issuers.cast(%{"provider" => "myspace"})
      assert {:error, "Firebase needs" <> _} = Issuers.cast(%{"provider" => "firebase"})

      assert {:error, "oidc needs an audience"} =
               Issuers.cast(%{"provider" => "oidc", "issuer" => "https://id.acme.com"})

      assert {:error, "this provider needs" <> _} = Issuers.cast(%{"provider" => "clerk"})

      assert {:error, "the issuer for clerk" <> _} =
               Issuers.cast(%{
                 "provider" => "clerk",
                 "issuer" => "https://clerk.acme.com/some/path"
               })

      assert {:ok, %{issuer: "https://id.acme.com/realms/acme"}} =
               Issuers.cast(%{
                 "provider" => "oidc",
                 "issuer" => "https://id.acme.com/realms/acme",
                 "audience" => "a"
               })

      assert {:ok,
              %{
                name_claim: "user_metadata.full_name",
                audience: "authenticated",
                issuer: "https://abc.supabase.co"
              }} =
               Issuers.cast(%{
                 "provider" => "supabase",
                 "issuer" => "https://abc.supabase.co/auth/v1"
               })

      base = %{"provider" => "clerk", "issuer" => "https://clerk.acme.com"}

      assert {:ok, %{room_claim: "org_id", room_match: "prefix"}} =
               Issuers.cast(
                 Map.put(base, "room_rule", %{"claim" => "org_id", "match" => "prefix"})
               )

      for bad <- [
            %{"claim" => "org_id", "match" => "regex"},
            %{"claim" => "org_id"},
            "org_id",
            %{"claim" => "a b", "match" => "prefix"},
            %{"claim" => "org_id", "match" => "prefix", "extra" => 1}
          ] do
        assert {:error, _} = Issuers.cast(Map.put(base, "room_rule", bad))
      end
    end

    test "hosted, only https to a public host" do
      Application.put_env(:presence_relay, :issuer_allow_local, false)
      on_exit(fn -> Application.put_env(:presence_relay, :issuer_allow_local, true) end)

      for bad <-
            ~w(http://clerk.acme.com https://localhost https://10.0.0.8 https://[::1] https://box.internal https://printer.local) do
        assert {:error, _} = Issuers.cast(%{"provider" => "clerk", "issuer" => bad}), bad
      end

      for bad <-
            ~w(http://example.com/ https://127.0.0.1/ https://169.254.169.254/ https://localhost/) do
        assert {:error, _} = Keys.fetchable(bad), bad
      end
    end
  end
end
