// The platform page (M150), at each route: the bar, then the page. Chat
// draws its own (the rooms view puts its search in the bar). Before anyone
// is signed in here it says where to sign in.

import { useEffect } from "preact/hooks";
import { chat } from "../shared/chat-store";
import { useSubscribe } from "../shared/ui/hooks";
import { MaintenanceNote } from "../shared/ui/maintenance-note";
import { PlatformBar } from "./bar";
import { names, resolve } from "./links";
import { go, pathOf, useRoute, type Route } from "./router";
import { controlOrigin, session } from "./session";
import { signIn } from "./signin";
import { toasts } from "./toast";
import { AgentsPage } from "./pages/agents";
import { ChatPage } from "./pages/chat";
import { Home } from "./pages/home";
import { InboxPage } from "./pages/inbox";
import { Missing, ProjectHome } from "./pages/project";
import { Installed, ProjectSettings } from "./pages/project-settings";
import { SettingsPage } from "./pages/settings";
import { PastPage, WorkspacesPage } from "./pages/workspace";

export function PlatformPage() {
  useSubscribe((fn) => session.subscribe(fn));
  const r = useRoute();
  useEffect(() => void session.load(), []);
  useSlug(r);
  useFind(r);
  const s = session.state;
  if (s === null) return <Shell><p class="platform-section dim">Loading…</p></Shell>;
  if (s === "signed-out") return <SignIn />;
  if (typeof s === "object") return <Shell><p class="platform-section control-error">{s.error}</p></Shell>;
  if (r.page === "chat") return <><ChatPage route={{ room: r.room, msg: r.msg, view: r.view, q: r.q }} /><Toast /></>;
  if (r.page === "dm") return <><ChatPage route={{ room: r.room, msg: r.msg }} /><Toast /></>;
  return <Shell>{body(r)}</Shell>;
}

function body(r: Route) {
  switch (r.page) {
    case "find":
      return <p class="platform-section dim">Finding it…</p>;
    case "home":
      return <Home />;
    case "project":
      return <ProjectHome id={r.project} />;
    case "workspaces":
      return <WorkspacesPage />;
    case "project-settings":
      return <ProjectSettings id={r.project} link={r.link} />;
    case "installed":
      return <Installed project={r.project} repo={r.repo} />;
    case "workspace":
      return <WorkspacesPage workspace={r.workspace} />;
    case "past":
      return <PastPage id={r.id} />;
    case "inbox":
      return <InboxPage />;
    case "agents":
      return <AgentsPage />;
    case "settings":
      return <SettingsPage />;
    default:
      return <Missing what="Nothing's at this address." />;
  }
}

function Shell({ children }: { children: preact.ComponentChildren }) {
  return (
    <div class="platform" data-platform>
      <PlatformBar />
      <MaintenanceNote />
      <main class="platform-main">{children}</main>
      <Toast />
    </div>
  );
}

function SignIn() {
  return (
    <div class="platform" data-platform data-platform-signed-out>
      <main class="platform-main">
        <section class="platform-section">
          <h1>Arugula</h1>
          <p>
            <button class="primary" data-sign-in onClick={() => void signIn(controlOrigin())}>
              Sign in
            </button>{" "}
            <span class="dim">on control's page, where your passkeys are; it brings you back here.</span>
          </p>
        </section>
      </main>
    </div>
  );
}

function Toast() {
  useSubscribe((fn) => toasts.subscribe(fn));
  const t = toasts.shown();
  return t ? (
    <div class="platform-toast" role="status" data-platform-toast>
      {t.text}
    </div>
  ) : null;
}

/** An id-only address (`/chat/:room`…) goes where it belongs as soon as
 * the page can say, in its place in history. */
function useFind(r: Route) {
  useSubscribe((fn) => chat.subscribe(fn));
  const to = r.page === "find" && session.state === "ready" ? resolve(r) : null;
  useEffect(() => {
    if (to) go(to, true);
  }, [to]);
}

/** Put the address's slugs right once the names are known: only the ids
 * are read, so an old slug lands, and the bar shows the current one. */
function useSlug(r: Route) {
  const want = r.page === "missing" || r.page === "find" ? null : pathOf(r, names);
  useEffect(() => {
    if (want && want !== location.pathname + location.search && session.state === "ready") go(want, true);
  }, [want, session.state]);
}
