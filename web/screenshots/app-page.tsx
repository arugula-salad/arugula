// The app as the site's pictures show it (`just screenshots`): the app's
// own pages (the platform's router, rail and pages, as on app.arugula.io)
// over the shots' daemon, as e2e/app-frame-page.tsx draws them for the
// end-to-end specs, with the platform's answers made up from ./team.ts:
// Game night's team, boards, issues and chat. The specs build this with
// Vite and serve it at /p/… on the daemon's origin, so its requests and
// sockets reach the daemon. `window.__shots.machines()` adds the
// teammates' machines to the page's fleet beside the daemon.

import { render } from "preact";
import "../src/shared/fonts.css";
import "../src/shared/style.css";
import "../src/shared/frame.css";
import "../src/platform/platform.css";
import "../src/platform/swarm.css";
import "../src/shared/ui/settings-section.css";
import "../src/platform/settings.css";
import "../src/platform/finish.css";
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
import { localBackend, setSeenBackend } from "../src/shared/seen";
import { session } from "../src/platform/session";
import { signedInOf } from "../src/app/signed-in";
import { setSignedIn } from "../src/shell/signed-in";
import { setPlatformHere } from "../src/shell/platform-links";
import { followPlatformLinks } from "../src/app/platform-here";
import { boards, cardOf, HERE, issues, LABELS, MACHINES, ME, messages, P, PEOPLE, PERSONAL, room, ROOMS, TEAM, view } from "./team";

await startPageKey();

const now = Date.now();
/** Launch v1's lead, a session on the shots' daemon, as the spec names it. */
const lead = JSON.parse(sessionStorage.getItem("shots.lead") ?? "null") ?? undefined;

const json = (body: unknown, status = 200) => new Response(JSON.stringify(body), { status, headers: { "Content-Type": "application/json" } });
const realFetch = window.fetch.bind(window);
window.fetch = async (input, init) => {
  const url = new URL(typeof input === "string" ? input : input instanceof URL ? input.href : input.url, location.href);
  const path = url.pathname;
  const method = (init?.method ?? (input instanceof Request ? input.method : "GET")).toUpperCase();
  let m: RegExpExecArray | null;
  if (path === "/api/projects")
    return json({
      me: { account: ME, login: "priya", name: "Priya" },
      projects: [
        { id: PERSONAL, name: "Priya", owner: { kind: "personal" }, team: null },
        { id: P, name: "Game night", owner: { kind: "team", id: TEAM, name: "Dice & Co" }, team: { id: TEAM, name: "Dice & Co", role: "owner", members: PEOPLE } },
      ],
      hosted: false,
    });
  // What wants Priya: Jen's PR for review, and an issue of hers.
  if (path === "/api/inbox") {
    const entry = (n: number, kind: string, reason: string, title: string) => ({ item: kind === "pr" ? `gamenight/app#${n}` : `iss_${n}`, reason, repo: "gamenight/app", number: n, kind, state: "open", title, author: "jen", url: "", project: P, seen_at: null });
    return json({ entries: [entry(41, "pr", "review_requested", "Vote on which games to play"), entry(14, "issue", "assigned", "Fix the flaky session expiry test")] });
  }
  // The daemon itself, by the id control would give it (and no address, so
  // it isn't listed twice): the Overview's Work level reaches its panes by
  // it. Only there: the machine page would take it for a far machine.
  if (path === "/api/hosts" && !location.pathname.includes("/machines")) {
    const list = (await (await realFetch(input, init)).json()) as { this: string; hosts: unknown[] };
    return json({ ...list, hosts: [...list.hosts, { name: list.this, id: HERE, urls: [], transport: "tailnet", added_ms: 0, last_seen_ms: null }] });
  }
  if (path === "/api/deploys/waiting") return json({ deploys: [] });
  if (/^\/api\/(projects|owners)\/[^/]+\/settings$/.test(path)) return json([]);
  if (/^\/api\/projects\/[^/]+\/sessions\/settings$/.test(path)) return json({ model: null, models: ["opus", "sonnet"] });
  if (path === "/api/sessions/pins") return json({ pins: [] });
  if (path === "/api/grants") return json({ grants: [] });

  // Boards.
  if (path === `/api/projects/${P}/boards`) return json({ project: P, boards: boards(now, lead) });
  if (path === `/api/projects/${P}/boards/views`) return json(boards(now, lead).map((b) => view(b.id, now, lead)));
  if ((m = /^\/api\/boards\/(brd_[0-9a-f]+)$/.exec(path))) return json(view(m[1], now, lead));
  if (path === `/api/projects/${P}/sessions/by-card`) return json({ project: P, groups: [] });
  if (path === `/api/projects/${P}/trackers`) return json({ connections: [] });
  if (path === `/api/projects/${P}/work`) return json({ project: P, items: [], next: null, total: 0, labels: [] });
  // The briefing: since last night, two merged and a PR waiting on you.
  if (path === `/api/projects/${P}/overview/seen`) return json({ seq: 100, at_ms: now - 15 * 3_600_000 });
  if (path === `/api/projects/${P}/events`) {
    const ev = (seq: number, type: string, pr: number) => ({ id: `evt_${seq}`, seq, type, v: 1, source: "github", scope: { team: TEAM, project: P }, subject: `gamenight/app#${pr}`, actor: null, at_ms: now - (130 - seq) * 600_000, recorded_ms: now, data: { repo: "gamenight/app", pr, branch: `b${pr}` }, key: `k${seq}` });
    return json({ events: [ev(101, "github.pr.merged", 38), ev(104, "github.pr.merged", 39), ev(110, "github.pr.ready", 41)], next_after: 120, head: 120 });
  }

  // Issues.
  if (path === `/api/projects/${P}/issues`) return json({ project: P, issues: issues(now, url.searchParams.get("state") ?? "open"), more: false });
  if (path === `/api/projects/${P}/issue-home`) return json({ project: P, name: "Game night", home: "native", chosen: true, repos: [], native_issues: true, both: false, labels: LABELS, team: TEAM });
  if ((m = /^\/api\/issues\/iss_(\d+)$/.exec(path))) {
    const i = issues(now, "all").find((x) => x.number === Number(m![1]));
    return i ? json(i) : json({ error: "not found" }, 404);
  }

  // Chat.
  if (path === "/api/chat") return json({ rooms: ROOMS.map((r) => room(r)), threads_unread: 0, threads_mentions: 0 });
  if (path === "/api/chat/sidebar") return json({ error: "none" }, 404);
  if (path === "/api/chat/presence")
    return json({ mode: "auto", people: PEOPLE.map((p, i) => ({ account: p.account, state: i === 3 ? "away" : "active" })), machines_online: [] });
  if ((m = /^\/api\/chat\/rooms\/([^/]+)(\/messages|\/read)?$/.exec(path))) {
    const r = ROOMS.find((x) => x.id === m![1]);
    if (!r) return json({ error: "no such room" }, 404);
    if (m[2] === "/messages") return json({ room: room(r), messages: messages(r, now), more: false });
    if (m[2] === "/read") return json({});
    return json({ room: room(r), people: PEOPLE.map((p) => ({ account: p.account, name: p.name, dnd: false })), can_manage: true, agents: [] });
  }
  if (path.startsWith("/api/chat") || path.startsWith("/api/notify")) return json({ error: "not here" }, 404);
  void method;
  return realFetch(input, init);
};

setSignedIn(signedInOf(session));
setPlatformHere((path, replace) => go(path, replace));
followPlatformLinks((path) => go(path), "[data-platform-machines]");
setSeenBackend(localBackend());

/** The teammates' machines in the page's fleet, beside the daemon, as
 * their summaries would come (e2e/machines-page.spec.ts' way). */
function machines() {
  const fleet = (window as unknown as { __app: { fleet: { inject(p: unknown[]): void } } }).__app.fleet;
  const proto = Object.getPrototypeOf(fleet);
  const real = (k: string) => Object.getOwnPropertyDescriptor(proto, k)!.get!;
  const project = { id: P, name: "Game night" };
  const hosts = MACHINES.map((h) => {
    const ids = [...h.agents.map((a) => a[0]), ...h.shells.map((s) => s[0])];
    return {
      name: h.name,
      id: h.id,
      transport: "relay",
      status: h.online ? "online" : "offline",
      state: h.online ? "connected" : "offline",
      ...(h.owner ? { owner: h.owner, ownerId: `acct_${h.owner.toLowerCase()}` } : {}),
      lastSeen: null,
      path: null,
      summary: {
        rev: 1,
        sessions: [{ id: 1, name: "game-night", tabs: [10], project }],
        tabs: [{ id: 10, name: null, layout: { panes: ids.map((p) => [p, { x: 0, y: 0, w: 1, h: 1 }]), splits: [] } }],
        panes: [],
        machines: [],
        options: {},
      },
    };
  });
  const home = (h: (typeof MACHINES)[number]) => `/Users/${(h.owner ?? "deploy").toLowerCase().replace("á", "a")}/src/game-night`;
  const pane = (h: (typeof MACHINES)[number], id: number, info: Record<string, unknown>) => ({
    key: `${h.name}:${id}`,
    host: h.name,
    id,
    info: { id, cwd: home(h), session_project: project, activity: { last_ms: now - id * 40_000, rate: 0.6 }, ...info },
    session: { id: 1, name: "game-night" },
    stale: !h.online,
    owner: h.owner,
    person: h.owner ? { id: `account:acct_${h.owner.toLowerCase()}`, name: h.owner, kind: "person" } : { id: "team", name: "Game night", kind: "team" },
    driver: null,
    watchers: [],
    unread: 0,
    mention: false,
  });
  const panes = MACHINES.flatMap((h) => [
    ...h.agents.map(([id, n, title, attention]) => {
      const card = cardOf(n)!;
      return pane(h, id, {
        type: "agent",
        command: "claude",
        title: `#${n} ${title}`,
        attention,
        branch: `${n}-${title.toLowerCase().replace(/[^a-z]+/g, "-").replace(/-$/, "")}`,
        on_issue: { issue: `#${n}`, project: P, boards: [{ project: P, board: card.board, title: card.title, row: card.row }] },
      });
    }),
    ...h.shells.map(([id, command]) => pane(h, id, { type: "terminal", command, attention: "working", branch: "main" })),
  ]);
  Object.defineProperty(fleet, "list", { configurable: true, get: () => [...real("list").call(fleet), ...hosts] });
  Object.defineProperty(fleet, "panes", { configurable: true, get: () => [...real("panes").call(fleet).filter((p: { key: string }) => !panes.some((f) => f.key === p.key)), ...panes] });
  fleet.inject([]);
}
(window as unknown as { __shots: unknown }).__shots = { machines };

const root = document.getElementById("root")!;
root.classList.add("frame");
render(
  <PlatformApiContext.Provider
    value={{ ...platformApi, Machines: appMachines(() => null), Overview: appOverview(() => null), ownPane: appOwnPane(() => null), ...appLive(() => null, (m) => (m === HERE ? "" : undefined)), MachineSettings: appMachineSettings(() => null, (m) => (m === HERE ? "" : undefined)), goToPane: (_machine, pane) => go(`/p/${P}/machines?pane=${pane}`) }}
  >
    <PlatformPage />
    <MenuLayer />
    <PromptLayer />
  </PlatformApiContext.Provider>,
  root,
);
