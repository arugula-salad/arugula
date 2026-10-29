// The signed-in owner's apps, and a new one: a name is all it takes. A
// new app comes with a live key; its page opens with the server secret
// shown, the one time it can be.

import { useEffect, useState } from "preact/hooks";
import * as api from "./api";
import { accountName, go, path, type Account, type App } from "./api";
import { value } from "./ui";

/** A secret just made, for the app page to show once (then forgotten). */
export const fresh: { id: string; secret: string } = { id: "", secret: "" };

function NewApp({ first }: { first: boolean }) {
  const [name, setName] = useState("");
  const [err, setErr] = useState("");
  const [busy, setBusy] = useState(false);
  const make = async () => {
    if (busy) return;
    setBusy(true);
    try {
      const { app, secret } = await api.createApp(name.trim());
      fresh.id = app.id;
      fresh.secret = secret;
      go(path(`/apps/${app.id}`));
    } catch (e) {
      setErr((e as Error).message);
      setBusy(false);
    }
  };
  return (
    <section class="card panel" style={{ "--c": "var(--k-server)" }}>
      <h2>{first ? "Make your first app" : "Make an app"}</h2>
      <p class="sub">One app for each product you add presence to. You can make a second one for staging.</p>
      <form
        class="row"
        onSubmit={(e) => {
          e.preventDefault();
          void make();
        }}
      >
        <input type="text" placeholder="My product" aria-label="The app's name" maxLength={80} value={name} onInput={(e) => setName(value(e))} />
        <button class="btn primary" type="submit" disabled={!name.trim() || busy}>
          Make the app
        </button>
      </form>
      {err && <span class="error">{err}</span>}
    </section>
  );
}

const plural = (n: number, one: string, many = `${one}s`) => `${n} ${n === 1 ? one : many}`;

export function AppsPage({ me }: { me: Account }) {
  const [list, setList] = useState<App[] | null>(null);
  const [err, setErr] = useState("");
  useEffect(() => {
    api.apps().then(setList, (e: Error) => setErr(e.message));
  }, []);
  return (
    <main class="apps-page">
      <h1 class="title">Hi, {accountName(me)}</h1>
      {err && <p class="error">{err}</p>}
      {list && list.length > 0 && (
        <section class="apps" aria-label="Your apps">
          <h2>Your apps</h2>
          <ul>
            {list.map((a) => (
              <li key={a.id}>
                <a class="card app-card" style={{ "--c": "var(--k-agent)" }} href={path(`/apps/${a.id}`)} onClick={(e) => (e.preventDefault(), go(path(`/apps/${a.id}`)))}>
                  <b>{a.name}</b>
                  <span class="stats">
                    {plural(a.keys.length, "key")} · {a.origins.length ? plural(a.origins.length, "origin") : "no origins yet"}
                    {a.guests ? " · guests welcome" : ""}
                  </span>
                  <code class="id">{a.id}</code>
                </a>
              </li>
            ))}
          </ul>
        </section>
      )}
      {list && <NewApp first={list.length === 0} />}
      {list && list.length === 0 && (
        <ol class="steps">
          <li>
            <b>Make an app.</b> It comes with a key you can put in your page.
          </li>
          <li>
            <b>Say where your pages live.</b> Keys only work from the sites you list (and test keys from your own computer).
          </li>
          <li>
            <b>Connect your sign-in.</b> Clerk, Auth0, Firebase, Supabase or any OpenID provider, so your people come in as themselves.
          </li>
          <li>
            <b>Paste the snippet.</b> Each app has one ready, with its own key.
          </li>
        </ol>
      )}
    </main>
  );
}
