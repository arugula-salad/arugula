// arugula.stream's dashboard (#318 P10): where a customer signs in, makes
// apps, copies their keys, says where their pages live and how their
// people sign in, and sees what they've used. /dashboard lists the apps;
// /dashboard/apps/<id> is one app. Served by the relay under /dashboard
// (api.ts's BASE), on the same session cookie as meet.

import { render } from "preact";
import { useEffect, useState } from "preact/hooks";
import { BASE, accountName, go, me as fetchMe, path, providers as fetchProviders, signOut, type Account, type Providers } from "./api";
import { AppsPage } from "./apps";
import { AppPage } from "./app";
import { SignInPage } from "./signin";
import "./style.css";

/** The app's id in the path (`<base>/apps/<id>`), or "". */
function appOf(pathname: string): string {
  const m = /^\/apps\/([^/]+)\/?$/.exec(pathname.slice(BASE.length));
  return m ? decodeURIComponent(m[1]!) : "";
}

/** arugula.stream's name and the dashboard's, linking to the apps. */
export function Top({ me, onSignedOut }: { me: Account | null; onSignedOut: () => void }) {
  return (
    <header class="top">
      <a class="mark" href="/">
        arugula<span>.</span>stream
      </a>
      <a class="where" href={path("/")} onClick={(e) => (e.preventDefault(), go(path("/")))}>
        dashboard
      </a>
      <nav class="who">
        <a href="/docs">Docs</a>
        {me && (
          <>
            <span class="stats" title={me.email ?? ""}>
              {me.email ?? accountName(me)}
            </span>
            <button
              class="btn small"
              onClick={async () => {
                await signOut().catch(() => undefined);
                onSignedOut();
              }}
            >
              Sign out
            </button>
          </>
        )}
      </nav>
    </header>
  );
}

function App() {
  const [where, setWhere] = useState(location.pathname);
  const [me, setMe] = useState<Account | null | undefined>(undefined);
  const [ways, setWays] = useState<Providers>({ email: false, google: false });
  const [failed, setFailed] = useState("");
  const load = () =>
    Promise.all([fetchMe(), fetchProviders()])
      .then(([m, w]) => (setMe(m), setWays(w), setFailed("")))
      .catch((e: Error) => setFailed(e.message));
  useEffect(() => {
    void load();
    const moved = () => setWhere(location.pathname);
    addEventListener("popstate", moved);
    return () => removeEventListener("popstate", moved);
  }, []);
  if (failed)
    return (
      <main class="lobby">
        <h2>The dashboard isn't answering</h2>
        <p class="sub">{failed}</p>
        <button class="btn" onClick={() => void load()}>
          Try again
        </button>
      </main>
    );
  if (me === undefined) return <main class="lobby" aria-busy="true" />;
  const signedOut = () => (setMe(null), go(path("/")));
  const id = appOf(where);
  return (
    <div class="page">
      <Top me={me} onSignedOut={signedOut} />
      {!me ? <SignInPage ways={ways} returnTo={where.startsWith(BASE) ? where : path("/")} /> : id ? <AppPage key={id} id={id} /> : <AppsPage me={me} />}
    </div>
  );
}

render(<App />, document.getElementById("app")!);
