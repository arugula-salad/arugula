// The fixture page for the app's frame (Arugula #989): the app's own pages
// (the platform's router, rail and pages) with its machine places
// (`appMachines`), over the suite's daemon instead of the hub: the app's
// machines with no session are a daemon's own page's, reached directly.
// Only the platform's project list is made up. app-frame.spec.ts builds
// this with Vite and serves it at /p/prj_aaaa0002/… on the daemon's
// origin, so its requests and sockets reach the daemon.

import { render } from "preact";
import type { InboxItem } from "../src/shared/proto.gen";
import "../src/shared/fonts.css";
import "../src/shared/style.css";
import "../src/shared/frame.css";
import "../src/platform/platform.css";
import "../src/platform/swarm.css";
import "../src/shared/ui/settings-section.css";
import "../src/platform/settings.css";
import "../src/shared/fields.css";
import "../src/shared/buttons.css";
import { startPageKey } from "../src/shell/local-key";
import { PlatformApiContext } from "../src/shared/platform-api";
import { MenuLayer, PromptLayer } from "../src/shared/ui/menu";
import { platformApi } from "../src/platform/api";
import { PlatformPage } from "../src/platform/page";
import { go } from "../src/platform/router";
import { appMachines, appOverview, appOwnPane } from "../src/app/machines";
import { appLive } from "../src/app/live";
import { appMachineSettings } from "../src/app/settings";
import { chat } from "../src/shared/chat-store";
import { localBackend, setSeenBackend } from "../src/shared/seen";
import { session } from "../src/platform/session";
import { signedInOf } from "../src/app/signed-in";
import { setSignedIn } from "../src/shell/signed-in";
import { setPlatformHere } from "../src/shell/platform-links";
import { followPlatformLinks } from "../src/app/platform-here";
import { setHomeMachine } from "../src/shell/pins";

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
        { id: "prj_aaaa0001", name: "Default", owner: { kind: "personal" }, team: null },
        { id: "prj_aaaa0002", name: "Arugula", owner: { kind: "team", id: "tm_1", name: "Arugula team" }, team: { id: "tm_1", name: "Arugula team", role: "owner", members: [{ account: "acct_a", name: "Sam", role: "owner" }, { account: "acct_b", name: "Jo", role: "editor" }] } },
        { id: "prj_aaaa0003", name: "Site", owner: { kind: "team", id: "tm_1", name: "Arugula team" }, team: null },
      ],
      hosted: false,
    });
  // What wants you: three things in Arugula, and one of no project's, as
  // the one Inbox's items (Arugula #1433), or the spec's own
  // (`window.__inbox`); with `project`, that project's alone. The count
  // and its split by project follow the server's rule: counted, and for
  // the reader (acct_a) or no one.
  if (url.pathname === "/api/inbox") {
    const project = url.searchParams.get("project");
    const item = (n: number, project: string | null) => ({ id: `iss_${n}:assigned`, kind: "assigned", group: "named", counts: true, title: `#${n} Thing ${n}`, from: {}, subject: { type: "issue", id: `iss_${n}`, url: "", ...(project ? { project } : {}) }, since: n });
    const all: InboxItem[] = (window as unknown as { __inbox?: InboxItem[] }).__inbox ?? [item(1, "prj_aaaa0002"), item(2, "prj_aaaa0002"), item(3, "prj_aaaa0002"), item(4, null)];
    const items = all.filter((i) => !project || i.subject.project === project);
    const by_project: Record<string, number> = {};
    let count = 0;
    for (const i of items) {
      if (!i.counts || (i.for && i.for !== "acct_a")) continue;
      count++;
      if (i.subject.project) by_project[i.subject.project] = (by_project[i.subject.project] ?? 0) + 1;
    }
    return json({ entries: [], items, count, by_project, handled: 0 });
  }
  if (url.pathname === "/api/deploys/waiting") return json({ deploys: [] });
  // A project's settings and its owner's defaults: none set.
  if (/^\/api\/(projects|owners)\/[^/]+\/settings$/.test(url.pathname)) return json([]);
  // The model its agent sessions use: the strongest, which follows the team's default.
  if (/^\/api\/projects\/[^/]+\/sessions\/settings$/.test(url.pathname)) return json({ model: null, models: ["opus", "sonnet"] });
  // The platform's chat and settings aren't this page's to show.
  if (url.pathname.startsWith("/api/chat") || url.pathname.startsWith("/api/notify")) return json({ error: "not here" }, 404);
  return realFetch(input, init);
};

// The spec seeds chat's rooms (the unread on Boards and Machines) through this.
(window as unknown as { __chat: typeof chat }).__chat = chat;
// A spec's Inbox (`/api/inbox` above), said as the stream says it changed.
(window as unknown as { __inboxSay(items: InboxItem[]): void }).__inboxSay = (items) => {
  (window as unknown as { __inbox: InboxItem[] }).__inbox = items;
  (chat as unknown as { heard(ev: unknown): void }).heard({ type: "inbox", count: 0, by_project: {}, changed: [] });
};

// Who's signed in, for the shell's blocks in a machine's pane: the
// platform's session (sam), as app/main.tsx says.
setSignedIn(signedInOf(session));
// And the platform is this page, as there: its links stay in it.
setPlatformHere((path, replace) => go(path, replace));
followPlatformLinks((path) => go(path), "[data-platform-machines]");

// Prefs (where you were) are this browser's here, as the app's are until
// the hub answers on its host.
setSeenBackend(localBackend());

/** The suite's daemon as Settings' This machine names it: a machine's id is hex. */
const SUITE = "5e771e0000000001";
// And what a pin names it by (session-pins.spec.ts stubs the pins).
setHomeMachine(SUITE);

// In the frame, as app.html's root is (shared/frame.css).
const root = document.getElementById("root")!;
root.classList.add("frame");
render(
  // A board card's session (board-live.spec.ts, overview.spec.ts): the machine `m_suite` is the suite's daemon, this page's origin.
  <PlatformApiContext.Provider
    value={{ ...platformApi, Machines: appMachines(() => null), Overview: appOverview(() => null), ownPane: appOwnPane(() => null), ...appLive(() => null, (m) => (m === "m_suite" ? "" : undefined)), MachineSettings: appMachineSettings(() => null, (m) => (m === SUITE ? "" : undefined)), goToPane: (_machine, pane) => go(`/p/prj_aaaa0002/machines?pane=${pane}`) }}
  >
    <PlatformPage />
    <MenuLayer />
    <PromptLayer />
  </PlatformApiContext.Provider>,
  root,
);
