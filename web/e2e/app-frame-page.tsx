// The fixture page for the app's frame (Arugula #989): the app's own pages
// (the platform's router, rail and pages) with its machine places
// (`appMachines`), over the suite's daemon instead of the hub: the app's
// machines with no session are a daemon's own page's, reached directly.
// Only the platform's project list is made up. app-frame.spec.ts builds
// this with Vite and serves it at /p/prj_aaaa0002/… on the daemon's
// origin, so its requests and sockets reach the daemon.

import { render } from "preact";
import "../src/shared/fonts.css";
import "../src/shared/style.css";
import "../src/shared/frame.css";
import "../src/platform/platform.css";
import "../src/platform/swarm.css";
import "../src/shared/ui/settings-section.css";
import "../src/platform/settings.css";
import "../src/platform/finish.css";
import "../src/shared/buttons.css";
import { startPageKey } from "../src/shell/local-key";
import { PlatformApiContext } from "../src/shared/platform-api";
import { MenuLayer, PromptLayer } from "../src/shared/ui/menu";
import { platformApi } from "../src/platform/api";
import { PlatformPage } from "../src/platform/page";
import { go } from "../src/platform/router";
import { appMachines, appOverview } from "../src/app/machines";
import { appLive } from "../src/app/live";
import { chat } from "../src/shared/chat-store";
import { localBackend, setSeenBackend } from "../src/shared/seen";

// The daemon's page key on this page's requests, as its own page has.
await startPageKey();

const json = (body: unknown, status = 200) => new Response(JSON.stringify(body), { status, headers: { "Content-Type": "application/json" } });
const realFetch = window.fetch.bind(window);
window.fetch = async (input, init) => {
  const url = new URL(typeof input === "string" ? input : input instanceof URL ? input.href : input.url, location.href);
  if (url.pathname === "/api/projects")
    return json({
      me: { account: "acct_a", login: "sam", name: "Sam" },
      projects: [
        { id: "prj_aaaa0001", name: "Personal", owner: { kind: "personal" }, team: null },
        { id: "prj_aaaa0002", name: "Arugula", owner: { kind: "team", id: "tm_1", name: "Arugula team" }, team: { id: "tm_1", name: "Arugula team", role: "owner", members: [] } },
        { id: "prj_aaaa0003", name: "Site", owner: { kind: "team", id: "tm_1", name: "Arugula team" }, team: null },
      ],
      hosted: false,
    });
  // What wants you: three things in Arugula, and a PR of a repo's (no project's).
  if (url.pathname === "/api/inbox") {
    const entry = (n: number, project: string | null) => ({ item: `iss_${n}`, reason: "assigned", repo: "acme/app", number: n, kind: "issue", state: "open", title: `Thing ${n}`, author: null, url: "", project, seen_at: null });
    return json({ entries: [entry(1, "prj_aaaa0002"), entry(2, "prj_aaaa0002"), entry(3, "prj_aaaa0002"), entry(4, null)] });
  }
  if (url.pathname === "/api/deploys/waiting") return json({ deploys: [] });
  // A project's settings and its owner's defaults: none set.
  if (/^\/api\/(projects|owners)\/[^/]+\/settings$/.test(url.pathname)) return json([]);
  // The platform's chat and settings aren't this page's to show.
  if (url.pathname.startsWith("/api/chat") || url.pathname.startsWith("/api/notify")) return json({ error: "not here" }, 404);
  return realFetch(input, init);
};

// The spec seeds chat's rooms (the unread on Boards and Sessions) through this.
(window as unknown as { __chat: typeof chat }).__chat = chat;

// Prefs (where you were) are this browser's here, as the app's are until
// the hub answers on its host.
setSeenBackend(localBackend());

// In the frame, as app.html's root is (shared/frame.css).
const root = document.getElementById("root")!;
root.classList.add("frame");
render(
  // A board card's session (board-live.spec.ts, overview.spec.ts): the machine `m_suite` is the suite's daemon, this page's origin.
  <PlatformApiContext.Provider
    value={{ ...platformApi, Machines: appMachines(() => null), Overview: appOverview(() => null), ...appLive(() => null, (m) => (m === "m_suite" ? "" : undefined)), goToPane: (_machine, pane) => go(`/p/prj_aaaa0002/machines?pane=${pane}`) }}
  >
    <PlatformPage />
    <MenuLayer />
    <PromptLayer />
  </PlatformApiContext.Provider>,
  root,
);
