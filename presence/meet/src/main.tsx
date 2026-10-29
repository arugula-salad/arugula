// meet.arugula.stream: our own app on arugula.stream's presence protocol,
// a place to hang out. The home page lets you in (a guest's name, or
// signing in) and lists a signed-in person's rooms; /r/<slug> is a room:
// everyone as a live character or outline, a chat beside them, waves and
// reactions. Served on a meet. host or under /meet (api.ts's BASE).

import { render } from "preact";
import { useEffect, useState } from "preact/hooks";
import { BASE, session, type Session } from "./api";
import { Home } from "./home";
import { RoomPage } from "./room";
import "./style.css";

/** The room's slug in the path (`<base>/r/<slug>`), or "". */
function slugOf(pathname: string): string {
  const m = /^\/r\/([^/]+)\/?$/.exec(pathname.slice(BASE.length));
  return m ? decodeURIComponent(m[1]!) : "";
}

function App() {
  const [where, setWhere] = useState(location.pathname);
  const [me, setMe] = useState<Session | null>(null);
  const [failed, setFailed] = useState("");
  const load = () =>
    session()
      .then((s) => (setMe(s), setFailed("")))
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
        <h2>meet isn't answering</h2>
        <p class="sub">{failed}</p>
        <button class="btn" onClick={() => void load()}>
          Try again
        </button>
      </main>
    );
  if (!me) return <main class="lobby" aria-busy="true" />;
  const slug = slugOf(where);
  return slug ? <RoomPage key={slug} slug={slug} me={me} /> : <Home me={me} onSignedOut={() => void load()} />;
}

render(<App />, document.getElementById("app")!);
