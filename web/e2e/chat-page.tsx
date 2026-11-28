// The fixture page for Chat in the app's frame (Lift 6c, Arugula #1485,
// and #1522's Agents section): the platform page itself (its router, rail,
// top bar and Chat) over a fake control, so no daemon or control is
// involved. A team project, Arugula, with three channels, two DMs, the
// reader's chief of staff, #agents and a channel per board (two of them
// closed). chat-page.spec.ts builds this with Vite and serves it at
// /p/prj_aaaa0002/…; `window.__fx` is the spec's hand.

import { render } from "preact";
import "../src/shared/style.css";
import "../src/shared/frame.css";
import "../src/platform/platform.css";
import "../src/shared/fields.css";
import "../src/shared/buttons.css";
import { PlatformApiContext } from "../src/shared/platform-api";
import { MenuLayer, PromptLayer } from "../src/shared/ui/menu";
import { apply as applyAppearance } from "../src/shared/appearance";
import { AppearanceLayer } from "../src/shared/ui/appearance";
import { platformApi } from "../src/platform/api";
import { PlatformPage } from "../src/platform/page";
import { chat } from "../src/shared/chat-store";
import { localBackend, setSeenBackend } from "../src/shared/seen";
import type { ChatMessage, ChatRoom, LayoutChange, PlaceLayout, ProjectBoard } from "../src/shared/proto.gen";
import { applyHere, blank } from "../src/shared/place-layout";
import type { ChatReached, ChatTag } from "../src/shared/chat/tags";

const P = "prj_aaaa0002";
const TEAM = "tm_1";
const T0 = Date.UTC(2026, 10, 27, 9, 0);

const room = (id: string, name: string, more: Partial<ChatRoom> = {}): ChatRoom => ({
  id,
  kind: "channel",
  team: TEAM,
  name,
  machine: null,
  machine_name: null,
  target: null,
  project: null,
  with: null,
  others: [],
  private: false,
  last: 3,
  read: 3,
  unread: 0,
  mentions: 0,
  wants: 0,
  muted: false,
  level: "all",
  level_own: false,
  archived: false,
  writable: true,
  topic: "",
  description: "",
  pins: 0,
  topic_writable: true,
  staff: false,
  ...more,
});

const person = (account: string, name: string) => ({ account, name, dnd: false });
const board = (id: string, title: string, lead: boolean, closed = false): ProjectBoard =>
  ({
    id,
    project: P,
    title,
    created_by: "acct_a",
    created_at: T0,
    updated_at: T0,
    goal: "",
    done_note: closed ? "done" : "",
    closed_by: closed ? "acct_a" : "",
    ...(closed ? { closed_at: T0 } : {}),
    progress: { done: 0, total: 0 },
    ...(lead ? { lead: { machine: "m_dust", machine_name: "dust", session: title, at: T0 } } : {}),
  }) as unknown as ProjectBoard;

const boardList: ProjectBoard[] = [
  board("brd_00000000000000000000000000000001", "Design lift", true),
  board("brd_00000000000000000000000000000002", "A2P", true),
  board("brd_00000000000000000000000000000003", "Product improvements", false),
  board("brd_00000000000000000000000000000004", "Rail and Machines", false),
  board("brd_00000000000000000000000000000005", "Old launch", false, true),
  board("brd_00000000000000000000000000000006", "Spring cleaning", false, true),
];

const rooms: ChatRoom[] = [
  room("ch_00000001", "general", { topic: "everyone on the team" }),
  room("ch_00000002", "design"),
  room("ch_00000003", "launch", { unread: 2, mentions: 1, last: 5 }),
  room("dm_00000001", "Jake", { kind: "dm", with: person("acct_b", "Jake"), others: [person("acct_b", "Jake")] }),
  room("dm_00000002", "Mira", { kind: "dm", with: person("acct_c", "Mira"), others: [person("acct_c", "Mira")] }),
  // The reader's chief of staff (#1275): a DM of them and their agents.
  room("dm_00000003", "Chief of staff", { kind: "dm", staff: true, project: P, others: [] }),
  // #agents: the project's own thread (#1522).
  room("ob_00000001", "agents", { kind: "object", team: null, target: P, project: P, topic: "work across boards: deploys, merge order, holds", unread: 1, last: 6 }),
  // Two boards' channels exist; the rest are made when first opened.
  room("ob_00000002", "Design lift", { kind: "object", team: null, target: boardList[0]!.id, project: P }),
  room("ob_00000003", "Product improvements", { kind: "object", team: null, target: boardList[2]!.id, project: P, unread: 2, wants: 2, last: 4 }),
  // An issue's comments: never listed in Chat.
  room("ob_00000009", "#1412", { kind: "object", team: null, target: "iss_00000000000000000000000000001412", project: P, unread: 1 }),
];

const msg = (seq: number, name: string, text: string, more: Partial<ChatMessage> = {}): ChatMessage => ({
  seq,
  at: T0 + seq * 7 * 60_000,
  author: name === "Sam" ? "acct_a" : `acct_${name.toLowerCase().replace(/\W/g, "")}`,
  name,
  text,
  quote: null,
  pane: null,
  mentions: [],
  edited: null,
  deleted: null,
  imported: null,
  agent: null,
  reactions: [],
  files: [],
  pinned: false,
  also: false,
  ...more,
});
const agent = (session: string) => ({ machine: "m_dust", machine_name: "dust", session });
const B1 = boardList[0]!.id;
const B2 = boardList[1]!.id;
/** The leads of the open boards. */
const LEADS = boardList.filter((b) => !b.closed_at && b.lead).map((b) => b.id);
/** A message's tags and who it reached (#1522's wire, shared/chat/tags). */
const said = (tags: ChatTag[], reached: ChatReached) => ({ tags, reached }) as Partial<ChatMessage>;

/** #1522's delivery rules, for a post of the reader's (a person's): in
 * #agents every lead, and the chief of staff when tagged; in a board's
 * channel its lead, and its workers for @here or @channel. */
function reachedBy(r: ChatRoom, tags: ChatTag[]): ChatReached {
  const cos = tags.some((t) => t.kind === "cos");
  if (r.target === P) return { leads: LEADS, all: true, ...(cos ? { cos } : {}) };
  const own = boardList.find((b) => b.id === r.target);
  const others = tags.flatMap((t) => (t.kind === "board" ? [t.board] : []));
  const workers = tags.some((t) => t.kind === "channel") ? 3 : tags.some((t) => t.kind === "here") ? 2 : 0;
  return { leads: [...(own?.lead ? [own.id] : []), ...others], ...(workers ? { workers } : {}), ...(cos ? { cos } : {}) };
}

const messages: Record<string, ChatMessage[]> = {
  ch_00000001: [msg(1, "Jake", "Morning. The pin goes out at noon.", { thread: { replies: 1, last: T0 + 20 * 60_000, repliers: [], following: false, read: 0, unread: 0, mentions: 0 } }), msg(2, "Sam", "Thanks. I'll watch the cloud specs."), msg(3, "Mira", "Docs are ready for review.")],
  ob_00000001: [
    msg(1, "Chief of staff", "**Holding merges** 10:00–12:00 for CI lab maintenance. Keep queueing PRs; I'll lift the hold at 12.", { agent: agent("Chief of staff"), ...said([], { leads: LEADS, all: true }) }),
    msg(2, "Launch: engineering", "@Design lift @A2P the cloud pin waits on `cloud#305`. Merge order once the hold lifts: #2920, then #2929.", {
      agent: agent("Launch: engineering"),
      thread: { replies: 3, last: T0 + 46 * 60_000, repliers: [], following: true, read: 0, unread: 0, mentions: 0 },
      ...said([{ kind: "board", board: B1, text: "Design lift" }, { kind: "board", board: B2, text: "A2P" }], { leads: [B1, B2] }),
    }),
    msg(3, "Onboarding", "@here is anyone touching `onboarding.spec.ts` today? #1412 changes its copy.", { agent: agent("Onboarding"), ...said([{ kind: "here", text: "here" }], { leads: [B1], active: true }) }),
    msg(4, "Rail and Machines", "#2418 merged: the rail caret sits under the project mark.", { agent: agent("Rail and Machines"), ...said([], { leads: [] }) }),
    msg(5, "Sam", "@cos let docs-only PRs through the hold.", said([{ kind: "cos", text: "cos" }], { leads: LEADS, all: true, cos: true })),
    msg(6, "Chief of staff", "Done. Docs-only PRs merge now; the rest wait for 12:30.", { agent: agent("Chief of staff"), ...said([], { leads: LEADS, all: true }) }),
  ],
};

const json = (body: unknown, status = 200) => new Response(JSON.stringify(body), { status, headers: { "Content-Type": "application/json" } });
window.fetch = async (input, init) => {
  const url = new URL(typeof input === "string" ? input : input instanceof URL ? input.href : input.url, location.href);
  const path = url.pathname;
  const method = init?.method ?? "GET";
  if (path === "/api/projects")
    return json({
      me: { account: "acct_a", login: "sam", name: "Sam" },
      projects: [
        { id: "prj_aaaa0001", name: "Default", owner: { kind: "personal" }, team: null },
        {
          id: P,
          name: "Arugula",
          owner: { kind: "team", id: TEAM, name: "Arugula team" },
          team: { id: TEAM, name: "Arugula team", role: "owner", members: [{ account: "acct_a", name: "Sam", role: "owner" }, { account: "acct_b", name: "Jake", role: "editor" }, { account: "acct_c", name: "Mira", role: "editor" }] },
        },
      ],
      hosted: false,
    });
  if (path === "/api/chat") return json({ rooms, threads_unread: 2, threads_mentions: 0 });
  if (path === "/api/chat/sidebar") return json({ starred: [], sections: [], collapsed: [], sorts: {} });
  if (path === `/api/projects/${P}/boards`) return json({ project: P, boards: boardList });
  const object = /^\/api\/chat\/targets$/.exec(path);
  if (object && method === "POST") {
    const { target } = JSON.parse(String(init!.body)) as { target: string };
    let r = rooms.find((x) => x.kind === "object" && x.target === target);
    if (!r) {
      const b = boardList.find((x) => x.id === target);
      r = room(`ob_${(rooms.length + 100).toString().padStart(8, "0")}`, b?.title ?? target, { kind: "object", team: null, target, project: P, last: 0, read: 0 });
      rooms.push(r);
    }
    return json(r);
  }
  if (path.endsWith("/watches")) return json({ watches: [] });
  const m = /^\/api\/chat\/rooms\/([^/]+)(?:\/(.+))?$/.exec(path);
  if (m) {
    const r = rooms.find((x) => x.id === m[1]);
    if (!r) return json({ error: "not found" }, 404);
    const list = (messages[r.id] ??= []);
    if (m[2] === "messages" && method === "POST") {
      const body = JSON.parse(String(init!.body)) as { text: string; tags?: ChatTag[]; parent?: number };
      window.__fx.posted.push(body);
      // A reply in a thread (#1569): kept with the thread's, counted on its first.
      const first = body.parent === undefined ? undefined : list.find((x) => x.seq === body.parent);
      if (first) {
        const thread = (window.__fx.replies[`${r.id}/${first.seq}`] ??= []);
        const seq = Math.max(list.at(-1)?.seq ?? 0, thread.at(-1)?.seq ?? 0) + 1;
        thread.push(msg(seq, "Sam", body.text, { parent: first.seq }));
        first.thread = { replies: thread.length, last: Date.now(), repliers: [], following: true, read: seq, unread: 0, mentions: 0 };
        return json({ seq, mentions: [], unreached: [], pushed: [] });
      }
      const seq = (list.at(-1)?.seq ?? 0) + 1;
      const tags = body.tags ?? [];
      list.push(msg(seq, "Sam", body.text, r.kind === "object" && /^(prj|brd)_/.test(r.target ?? "") ? said(tags, reachedBy(r, tags)) : {}));
      r.last = r.read = seq;
      return json({ seq, mentions: [], unreached: [], pushed: [] });
    }
    const thread = Number(url.searchParams.get("thread") ?? 0);
    if (m[2] === "messages" && thread) {
      const replies = (window.__fx.replies[`${r.id}/${thread}`] ?? []).filter((x) => x.seq > Number(url.searchParams.get("after") ?? 0));
      return json({ room: r, messages: replies, more: false, parent: list.find((x) => x.seq === thread) });
    }
    if (m[2] === "messages") {
      const after = Number(url.searchParams.get("after") ?? 0);
      return json({ room: r, messages: list.filter((x) => x.seq > after), more: false });
    }
    if (m[2]?.endsWith("/agents")) return json({ agents: [] });
    if (m[2] === "pins") return json({ pins: [] });
    if (m[2] === undefined) return json({ room: r, people: [person("acct_a", "Sam"), person("acct_b", "Jake")], can_manage: true, agents: [] });
    return json({});
  }
  // A board's page (Lift 5b), and its place layout (#1496) kept here.
  const bv = /^\/api\/boards\/(brd_[0-9a-f]+)$/.exec(path);
  if (bv && method === "GET") {
    const b = boardList.find((x) => x.id === bv[1]);
    return b ? json({ board: b, zones: (["next", "doing", "review", "done"] as const).map((lane) => ({ id: `lane_${lane}`, rev: 1, title: lane, kind: "lane", rows: [] })), groups: [], docs: [] }) : json({ error: "not found" }, 404);
  }
  const lay = /^\/api\/layouts\/(.+)$/.exec(path);
  if (lay) {
    const place = decodeURIComponent(lay[1]!);
    const now = (window.__fx.layouts[place] ??= blank(place));
    if (method === "GET") return json(now);
    const change = JSON.parse(String(init!.body)) as LayoutChange;
    window.__fx.layoutsPosted.push({ place, change });
    const r = applyHere(now, change);
    window.__fx.layouts[place] = r.layout;
    return json(r);
  }
  if (path === "/api/inbox") return json({ entries: [], items: [], count: 0, by_project: {}, handled: 0 });
  if (path.startsWith("/api/")) return json({ error: "not here" }, 404);
  return new Response("", { status: 404 });
};
// No live stream here: the page reads what it's given.
(window as unknown as { EventSource: unknown }).EventSource = class {
  close() {}
  addEventListener() {}
};

declare global {
  interface Window {
    __fx: {
      chat: typeof chat;
      rooms: ChatRoom[];
      boards: ProjectBoard[];
      replies: Record<string, ChatMessage[]>;
      posted: { text: string; tags?: ChatTag[] }[];
      layouts: Record<string, PlaceLayout>;
      layoutsPosted: { place: string; change: LayoutChange }[];
    };
  }
}
// The replies to Launch: engineering's post in #agents.
const replies: Record<string, ChatMessage[]> = {
  "ch_00000001/1": [msg(4, "Sam", "I'll watch it.", { parent: 1 })],
  "ob_00000001/2": [
    msg(7, "Design lift", "Ack. #2920 is green; it goes first.", { agent: agent("Design lift"), parent: 2 }),
    msg(8, "A2P", "#2929 rebases after it.", { agent: agent("A2P"), parent: 2 }),
    msg(9, "Jake", "Thanks both.", { parent: 2 }),
  ],
};
window.__fx = { chat, rooms, boards: boardList, replies, posted: [], layouts: {}, layoutsPosted: [] };

setSeenBackend(localBackend());
applyAppearance();
const root = document.getElementById("root")!;
root.classList.add("frame");
render(
  <PlatformApiContext.Provider value={platformApi}>
    <PlatformPage />
    <MenuLayer />
    <PromptLayer />
    <AppearanceLayer />
  </PlatformApiContext.Provider>,
  root,
);
