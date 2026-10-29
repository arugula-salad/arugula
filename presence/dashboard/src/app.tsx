// One app: its keys (live and test), a snippet to paste with its own key,
// the origins its keys work from, how its people sign in (and the room
// rule), whether guests may come, its usage, its plan, and the server secret, which
// most apps never need.

import { useEffect, useState } from "preact/hooks";
import * as api from "./api";
import { go, path, type App, type Issuer, type Key } from "./api";
import { fresh } from "./apps";
import { ProviderSection, RoomRuleSection } from "./provider";
import { Quickstart } from "./quickstart";
import { UsageSection } from "./usage";
import { BillingSection } from "./billing";
import { Confirm, CopyButton, Section, value } from "./ui";

const SECTIONS = [
  ["keys", "Keys"],
  ["quickstart", "Quickstart"],
  ["origins", "Origins"],
  ["sign-in", "Sign-in"],
  ["room-rule", "Room rule"],
  ["guests", "Guests"],
  ["usage", "Usage"],
  ["billing", "Plan"],
  ["secret", "Server secret"],
] as const;

/** A secret on screen the one time it can be, until they say they've kept it. */
function Secret({ secret, onDone, made }: { secret: string; onDone: () => void; made: "new" | "rotated" }) {
  return (
    <div class="secret card" style={{ "--c": "var(--k-build)" }} role="status">
      <b>{made === "new" ? "Your app's server secret" : "Your new server secret"}</b>
      <p class="sub">
        This is the only time we can show it. You only need it if your own server signs room tokens; keep it there, never in a page.
        {made === "rotated" && " The old secret stops working within a minute."}
      </p>
      <div class="row">
        <code class="mono secret-value" aria-label="The server secret">
          {secret}
        </code>
        <CopyButton text={secret} name="Copy the secret" />
        <button class="btn small" onClick={onDone}>
          I've kept it
        </button>
      </div>
    </div>
  );
}

function KeyRow({ k, onRevoke }: { k: Key; onRevoke: () => Promise<void> }) {
  return (
    <li class="key-row" data-mode={k.mode}>
      <span class={`tag ${k.mode}`}>{k.mode}</span>
      <code class="mono key" title={k.key}>
        {k.key}
      </code>
      <span class="actions">
        <CopyButton text={k.key} name={`Copy ${k.mode} key ${k.key}`} />
        <Confirm label="Revoke" name={`Revoke ${k.key}`} question="Revoke it? Pages using it stop connecting." yes="Revoke" onYes={onRevoke} />
      </span>
    </li>
  );
}

function KeysSection({ app, onChange }: { app: App; onChange: (a: App) => void }) {
  const [err, setErr] = useState("");
  const make = async (mode: "live" | "test") => {
    try {
      const k = await api.makeKey(app.id, mode);
      onChange({ ...app, keys: [...app.keys, k] });
      setErr("");
    } catch (e) {
      setErr((e as Error).message);
    }
  };
  const revoke = async (key: string) => {
    try {
      await api.revokeKey(app.id, key);
      onChange({ ...app, keys: app.keys.filter((k) => k.key !== key) });
      setErr("");
    } catch (e) {
      setErr((e as Error).message);
    }
  };
  const live = app.keys.filter((k) => k.mode === "live");
  const test = app.keys.filter((k) => k.mode === "test");
  return (
    <Section id="keys" title="Keys" kind="--k-server" sub="Publishable keys are safe to put in your page: they only work from your origins. Use a test key while you build; it also works from your own computer.">
      {!app.keys.length && <p class="empty">No keys. Make one to connect.</p>}
      <ul class="keys">
        {[...live, ...test].map((k) => (
          <KeyRow key={k.key} k={k} onRevoke={() => revoke(k.key)} />
        ))}
      </ul>
      <div class="row">
        <button class="btn small" onClick={() => void make("live")}>
          New live key
        </button>
        <button class="btn small" onClick={() => void make("test")}>
          New test key
        </button>
      </div>
      {err && <span class="error">{err}</span>}
    </Section>
  );
}

function OriginsSection({ app, onChange }: { app: App; onChange: (a: App) => void }) {
  const [draft, setDraft] = useState("");
  const [err, setErr] = useState("");
  const add = async () => {
    const o = draft.trim();
    if (!o) return;
    try {
      onChange(await api.addOrigin(app.id, o));
      setDraft("");
      setErr("");
    } catch (e) {
      setErr((e as Error).message);
    }
  };
  const remove = async (o: string) => {
    try {
      onChange(await api.removeOrigin(app.id, o));
    } catch (e) {
      setErr((e as Error).message);
    }
  };
  return (
    <Section
      id="origins"
      title="Origins"
      kind="--k-test"
      sub={
        <>
          The sites your pages are on. A key only connects from these. Write each as <code>https://app.example.com</code>: the scheme, the host, and
          a port if it isn't the usual one, with no path. <code>https://*.example.com</code> covers every subdomain, but not{" "}
          <code>example.com</code> itself. Test keys also work from <code>localhost</code> and <code>127.0.0.1</code> on any port, so you
          needn't list those.
        </>
      }
    >
      {!app.origins.length && <p class="empty">No origins yet: live keys won't connect from anywhere until you add one.</p>}
      <ul class="origins">
        {app.origins.map((o) => (
          <li key={o}>
            <code class="mono">{o}</code>
            <button class="btn small" onClick={() => void remove(o)} aria-label={`Remove ${o}`}>
              Remove
            </button>
          </li>
        ))}
      </ul>
      <form
        class="row"
        onSubmit={(e) => {
          e.preventDefault();
          void add();
        }}
      >
        <input type="text" inputMode="url" placeholder="https://app.example.com" aria-label="An origin" value={draft} onInput={(e) => setDraft(value(e))} />
        <button class="btn" type="submit" disabled={!draft.trim()}>
          Add the origin
        </button>
      </form>
      {err && (
        <span class="error" role="alert">
          {err}
        </span>
      )}
    </Section>
  );
}

function GuestsSection({ app, onChange }: { app: App; onChange: (a: App) => void }) {
  const [err, setErr] = useState("");
  const flip = async () => {
    try {
      onChange(await api.updateApp(app.id, { guests: !app.guests }));
      setErr("");
    } catch (e) {
      setErr((e as Error).message);
    }
  };
  return (
    <Section id="guests" title="Guests" kind="--k-editor" sub="Guests come in with just a name, no sign-in: good for public rooms, demos and trying things out. Anyone on your site could join any room as a guest, and room rules don't apply to them.">
      <button class="switch" role="switch" aria-checked={app.guests} onClick={() => void flip()}>
        <span class="knob" aria-hidden="true" />
        {app.guests ? "Guests can join" : "Signed-in people only"}
      </button>
      {err && <span class="error">{err}</span>}
    </Section>
  );
}

function SecretSection({ app }: { app: App }) {
  const [secret, setSecret] = useState("");
  const [err, setErr] = useState("");
  return (
    <Section
      id="secret"
      title="Server secret"
      kind="--k-build"
      sub={
        <>
          Only for apps whose own server signs room tokens (see <a href="/docs#tokens">Room tokens</a>). With a key and your sign-in provider, you
          can leave it alone. We keep it encrypted and can't show it again; if it's lost or leaked, make a new one.
        </>
      }
    >
      {secret ? (
        <Secret secret={secret} made="rotated" onDone={() => setSecret("")} />
      ) : (
        <div class="row">
          <Confirm
            label="Make a new secret"
            question="Make a new one? The old secret stops working within a minute, so update your server first."
            yes="Make a new secret"
            onYes={async () => {
              try {
                setSecret(await api.rotateSecret(app.id));
                setErr("");
              } catch (e) {
                setErr((e as Error).message);
              }
            }}
          />
        </div>
      )}
      {err && <span class="error">{err}</span>}
    </Section>
  );
}

export function AppPage({ id }: { id: string }) {
  const [app, setApp] = useState<App | null>(null);
  const [issuer, setIssuer] = useState<Issuer | null>(null);
  const [err, setErr] = useState("");
  const [secret, setSecret] = useState(fresh.id === id ? fresh.secret : "");
  useEffect(() => {
    // Shown once: a reload or a visit back doesn't bring it back.
    fresh.id = fresh.secret = "";
    Promise.all([api.getApp(id), api.issuer(id)]).then(
      ([a, i]) => (setApp(a), setIssuer(i)),
      (e: Error) => setErr(e.message),
    );
  }, [id]);
  if (err)
    return (
      <main class="app-page">
        <p class="error">{err === "no such app" ? "There's no app here, or it isn't yours." : err}</p>
        <a href={path("/")} onClick={(e) => (e.preventDefault(), go(path("/")))}>
          Back to your apps
        </a>
      </main>
    );
  if (!app) return <main class="app-page" aria-busy="true" />;
  return (
    <main class="app-page">
      <a class="back" href={path("/")} onClick={(e) => (e.preventDefault(), go(path("/")))}>
        ← Your apps
      </a>
      <header class="app-head">
        <h1 class="title">{app.name}</h1>
        <span class="stats">
          <code class="mono">{app.id}</code> <CopyButton text={app.id} name="Copy the app's id" />
        </span>
      </header>
      {secret && <Secret secret={secret} made="new" onDone={() => setSecret("")} />}
      <nav class="jump" aria-label="On this page">
        {SECTIONS.map(([s, t]) => (
          <a href={`#${s}`}>{t}</a>
        ))}
      </nav>
      <div class="sections">
        <KeysSection app={app} onChange={setApp} />
        <Quickstart app={app} issuer={issuer} />
        <OriginsSection app={app} onChange={setApp} />
        <ProviderSection app={app} issuer={issuer} onChange={setIssuer} />
        <RoomRuleSection app={app} issuer={issuer} onChange={setIssuer} />
        <GuestsSection app={app} onChange={setApp} />
        <UsageSection app={app} />
        <BillingSection app={app} />
        <SecretSection app={app} />
      </div>
    </main>
  );
}
