// How an app's people sign in (#318 P4): a preset (Clerk, Auth0, Firebase,
// Supabase, or any OpenID Connect provider) and the few values it needs,
// each with where to find it; the relay's own words when one's wrong,
// under the field they're about. And the room rule, which belongs to the
// provider (PUT /issuer carries both, so each form sends the other's
// settings back unchanged).

import { useEffect, useState } from "preact/hooks";
import * as api from "./api";
import type { App, Issuer, IssuerInput, Provider, RoomRule } from "./api";
import { Confirm, Field, Section, value } from "./ui";

interface Preset {
  label: string;
  /** The place field: the issuer URL, or Firebase's project id. */
  place: { key: "issuer" | "project"; label: string; placeholder: string; hint: string };
  audience: { required: boolean; placeholder: string; hint: string };
  name: string;
}

export const PRESETS: Record<Provider, Preset> = {
  clerk: {
    label: "Clerk",
    place: {
      key: "issuer",
      label: "Frontend API URL",
      placeholder: "https://clerk.example.com",
      hint: "In the Clerk Dashboard, under API keys: the Frontend API URL (https://….clerk.accounts.dev in development).",
    },
    audience: { required: false, placeholder: "leave empty", hint: "Clerk's session tokens carry none; leave it empty unless you've added one." },
    name: 'Clerk\'s session token has no name by default: under Sessions, customize it with "name": "{{user.full_name}}".',
  },
  auth0: {
    label: "Auth0",
    place: {
      key: "issuer",
      label: "Domain",
      placeholder: "acme.us.auth0.com",
      hint: "In the Auth0 Dashboard, your application's Settings: the Domain (or your custom domain).",
    },
    audience: {
      required: true,
      placeholder: "https://api.example.com",
      hint: "Under APIs, your API's Identifier: the audience you ask getTokenSilently() for. (Sending ID tokens instead? Your application's Client ID.)",
    },
    name: "Access tokens carry no name: pass name to connect(), or add a claim with an Auth0 Action and name it here.",
  },
  firebase: {
    label: "Firebase",
    place: {
      key: "project",
      label: "Project ID",
      placeholder: "my-project",
      hint: "In the Firebase console, Project settings, General: the Project ID (not its name or number).",
    },
    audience: { required: false, placeholder: "", hint: "" },
    name: "Firebase ID tokens carry the person's name when their sign-in method has one.",
  },
  supabase: {
    label: "Supabase",
    place: {
      key: "issuer",
      label: "Project URL",
      placeholder: "https://abcdefgh.supabase.co",
      hint: "In the Supabase dashboard, Project Settings, Data API: the Project URL. Your project needs asymmetric JWT signing keys (JWT Keys); the legacy shared secret isn't supported.",
    },
    audience: { required: false, placeholder: "authenticated", hint: "Leave it empty for Supabase's own: authenticated." },
    name: "Supabase keeps it in user_metadata.full_name, which is what we read unless you say otherwise.",
  },
  oidc: {
    label: "OpenID Connect",
    place: {
      key: "issuer",
      label: "Issuer URL",
      placeholder: "https://id.example.com/realms/acme",
      hint: "Exactly the iss in your tokens: the issuer in your provider's /.well-known/openid-configuration. We find its keys from there.",
    },
    audience: { required: true, placeholder: "your client id", hint: "The aud your tokens carry: usually your client ID for ID tokens, or your API's audience." },
    name: "Most providers put it in name.",
  },
};

type Fields = { issuer: string; project: string; audience: string; name_claim: string };
const blank: Fields = { issuer: "", project: "", audience: "", name_claim: "" };

/** Which field the relay's message is about ("" for none in particular). */
export function fieldOf(msg: string): keyof Fields | "rule" | "" {
  if (/room rule/.test(msg)) return "rule";
  if (/audience/.test(msg)) return "audience";
  if (/name_claim/.test(msg)) return "name_claim";
  if (/project/i.test(msg)) return "project";
  if (/issuer|URL/.test(msg)) return "issuer";
  return "";
}

/** An issuer as the relay shows it, as PUT takes it back. */
export function inputOf(i: Issuer): IssuerInput {
  return {
    provider: i.provider,
    ...(i.issuer ? { issuer: i.issuer } : {}),
    ...(i.project ? { project: i.project } : {}),
    ...(i.audience && i.provider !== "firebase" ? { audience: i.audience } : {}),
    name_claim: i.name_claim,
    room_rule: i.room_rule,
  };
}

function fieldsOf(i: Issuer | null): Fields {
  if (!i) return blank;
  const defaultAudience = i.provider === "firebase" || (i.provider === "supabase" && i.audience === "authenticated");
  return { issuer: i.issuer ?? "", project: i.project ?? "", audience: defaultAudience ? "" : (i.audience ?? ""), name_claim: i.name_claim };
}

const defaultName = (p: Provider) => (p === "supabase" ? "user_metadata.full_name" : "name");

export function ProviderSection({ app, issuer, onChange }: { app: App; issuer: Issuer | null; onChange: (i: Issuer | null) => void }) {
  const [provider, setProvider] = useState<Provider>(issuer?.provider ?? "clerk");
  const [f, setF] = useState<Fields>(fieldsOf(issuer));
  const [err, setErr] = useState<{ field: string; msg: string } | null>(null);
  const [saved, setSaved] = useState(false);
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    if (issuer) {
      setProvider(issuer.provider);
      setF(fieldsOf(issuer));
    }
  }, [issuer?.provider, issuer?.issuer, issuer?.project, issuer?.audience, issuer?.name_claim]);
  const p = PRESETS[provider];
  const set = (k: keyof Fields) => (e: Event) => (setF({ ...f, [k]: value(e) }), setSaved(false));
  const save = async () => {
    setBusy(true);
    const input: IssuerInput = { provider, room_rule: issuer?.room_rule ?? null };
    const place = f[p.place.key].trim();
    if (place) input[p.place.key] = place;
    if (provider !== "firebase" && f.audience.trim()) input.audience = f.audience.trim();
    if (f.name_claim.trim() && f.name_claim.trim() !== defaultName(provider)) input.name_claim = f.name_claim.trim();
    try {
      onChange(await api.putIssuer(app.id, input));
      setErr(null);
      setSaved(true);
    } catch (e) {
      const msg = (e as Error).message;
      setErr({ field: fieldOf(msg), msg });
      setSaved(false);
    } finally {
      setBusy(false);
    }
  };
  const errFor = (k: string) => (err?.field === k ? err.msg : undefined);
  return (
    <Section
      id="sign-in"
      title="Sign-in"
      kind="--k-agent"
      sub="Already signing people in? Name your provider, and your page joins rooms with the person's own sign-in token. We check it against your provider's published keys, so you need no server for this."
    >
      <div class="seg presets" role="radiogroup" aria-label="Your sign-in provider">
        {(Object.keys(PRESETS) as Provider[]).map((k) => (
          <button
            role="radio"
            aria-checked={provider === k}
            class={provider === k ? "btn small on" : "btn small"}
            onClick={() => {
              setProvider(k);
              setErr(null);
              setSaved(false);
            }}
          >
            {PRESETS[k].label}
          </button>
        ))}
      </div>
      <form
        class="fields"
        onSubmit={(e) => {
          e.preventDefault();
          void save();
        }}
      >
        <Field label={p.place.label} hint={p.place.hint} error={errFor(p.place.key)}>
          <input type="text" spellcheck={false} placeholder={p.place.placeholder} value={f[p.place.key]} onInput={set(p.place.key)} />
        </Field>
        {provider !== "firebase" && (
          <Field label={p.audience.required ? "Audience" : "Audience (optional)"} hint={p.audience.hint} error={errFor("audience")}>
            <input type="text" spellcheck={false} placeholder={p.audience.placeholder} value={f.audience} onInput={set("audience")} />
          </Field>
        )}
        <details class="more" open={!!errFor("name_claim") || (!!f.name_claim && f.name_claim !== defaultName(provider))}>
          <summary>Where their name is</summary>
          <Field label="Name claim" hint={`${p.name} A dotted name (a.b) looks inside an object.`} error={errFor("name_claim")}>
            <input type="text" spellcheck={false} placeholder={defaultName(provider)} value={f.name_claim} onInput={set("name_claim")} />
          </Field>
        </details>
        {err && !["issuer", "project", "audience", "name_claim"].includes(err.field) && (
          <span class="error" role="alert">
            {err.msg}
          </span>
        )}
        <div class="row">
          <button class="btn primary" type="submit" disabled={busy}>
            {issuer ? "Save changes" : `Use ${p.label}`}
          </button>
          {saved && (
            <span class="ok" role="status">
              Saved.
            </span>
          )}
          {issuer && (
            <Confirm
              label="Stop using it"
              question="Signed-in people stop getting in. Sure?"
              yes="Stop using it"
              onYes={async () => {
                await api.deleteIssuer(app.id);
                onChange(null);
                setF(blank);
                setSaved(false);
              }}
            />
          )}
        </div>
      </form>
      {issuer && (
        <p class="now">
          Now: <b>{PRESETS[issuer.provider].label}</b>. Tokens must come from <code>{issuer.iss}</code>
          {issuer.audience && (
            <>
              {" "}
              for <code>{issuer.audience}</code>
            </>
          )}
          .
        </p>
      )}
    </Section>
  );
}

export function RoomRuleSection({ app, issuer, onChange }: { app: App; issuer: Issuer | null; onChange: (i: Issuer | null) => void }) {
  const [claim, setClaim] = useState(issuer?.room_rule?.claim ?? "");
  const [match, setMatch] = useState<RoomRule["match"]>(issuer?.room_rule?.match ?? "prefix");
  const [err, setErr] = useState("");
  const [saved, setSaved] = useState(false);
  useEffect(() => {
    setClaim(issuer?.room_rule?.claim ?? "");
    setMatch(issuer?.room_rule?.match ?? "prefix");
  }, [issuer?.room_rule?.claim, issuer?.room_rule?.match]);
  const put = async (rule: RoomRule | null) => {
    if (!issuer) return;
    try {
      onChange(await api.putIssuer(app.id, { ...inputOf(issuer), room_rule: rule }));
      setErr("");
      setSaved(true);
    } catch (e) {
      setErr((e as Error).message);
      setSaved(false);
    }
  };
  const example = claim.trim() || (match === "prefix" ? "org_id" : "rooms");
  return (
    <Section
      id="room-rule"
      title="Room rule"
      kind="--k-agent"
      sub="Without one, anyone signed in can join any room. A rule keeps each person to their rooms, by a claim in their token. Guests aren't held to it."
    >
      {!issuer ? (
        <p class="empty">Set your sign-in provider first: a rule reads its tokens.</p>
      ) : (
        <form
          class="fields"
          onSubmit={(e) => {
            e.preventDefault();
            void put({ claim: claim.trim(), match });
          }}
        >
          <div class="seg" role="radiogroup" aria-label="How the rule matches">
            <button type="button" role="radio" aria-checked={match === "prefix"} class={match === "prefix" ? "btn small on" : "btn small"} onClick={() => (setMatch("prefix"), setSaved(false))}>
              Room starts with it
            </button>
            <button type="button" role="radio" aria-checked={match === "contains"} class={match === "contains" ? "btn small on" : "btn small"} onClick={() => (setMatch("contains"), setSaved(false))}>
              Room is in a list
            </button>
          </div>
          <Field
            label="Claim"
            error={err || undefined}
            hint={
              match === "prefix" ? (
                <>
                  Someone whose <code>{example}</code> is <code>acme</code> may join <code>acme:standup</code> and any other <code>acme:</code> room, and
                  no other.
                </>
              ) : (
                <>
                  Someone whose <code>{example}</code> is <code>["standup", "design"]</code> may join those two rooms and no other.
                </>
              )
            }
          >
            <input type="text" spellcheck={false} placeholder={match === "prefix" ? "org_id" : "rooms"} aria-label="The claim" value={claim} onInput={(e) => (setClaim(value(e)), setSaved(false))} />
          </Field>
          <div class="row">
            <button class="btn primary" type="submit" disabled={!claim.trim()}>
              Save the rule
            </button>
            {saved && (
              <span class="ok" role="status">
                Saved.
              </span>
            )}
            {issuer.room_rule && (
              <button type="button" class="btn small" onClick={() => void put(null).then(() => setClaim(""))}>
                Remove the rule
              </button>
            )}
          </div>
        </form>
      )}
    </Section>
  );
}
