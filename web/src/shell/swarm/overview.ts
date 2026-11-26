// A project's Overview (Arugula inversion, L6): Swarm for one project,
// one row per open board (#1085), then the cards in flight (Doing and
// Review) under it, each holding the sessions on it, and the sessions on no
// card last.
// Plain functions from the project's boards, the panes the machines report
// and the hub's sessions by card, for the app's view and its test
// (scripts/overview.test.mjs). A pane finds its card as a board's focus
// does (./board-focus.ts): its agent's `on_issue`, or a row's attachment
// of its block; then by the hub's sessions by card, and by its session (a
// session's card is its panes' card).

import type { BoardLead, BoardRow, BoardView, OnIssue } from "../../shared/proto";
import { hubStanding, panesOf, type SessionsByCard, type ProjectSession } from "../../shared/project-sessions.ts";
import { chiefProject, sessionLabel } from "../../shared/chief-of-staff.ts";

/** How a card or a session stands, most urgent first: a session waits on
 * you, the row's question asks you, a session failed, its PR's checks fail,
 * it or its question waits on someone else (`waits`, #1392), it's quiet,
 * it's working, its PR's checks run, it's in review; or nothing says
 * (`idle`). A session only ever needs you, failed, waits on someone else,
 * is quiet or works. */
export type Standing = "needs" | "asks" | "failed" | "checks_failing" | "waits" | "quiet" | "working" | "checks_running" | "review" | "idle";
export const STANDING_ORDER: Standing[] = ["needs", "asks", "failed", "checks_failing", "waits", "quiet", "working", "checks_running", "review", "idle"];
/** The one word a row says of how it stands; nothing for `idle`. */
export const STATE_WORD: Record<Standing, string> = {
  needs: "needs you",
  asks: "asks you",
  failed: "failed",
  checks_failing: "checks failing",
  waits: "asks someone else",
  quiet: "quiet",
  working: "working",
  checks_running: "checks running",
  review: "in review",
  idle: "",
};
const rank = (s: Standing) => STANDING_ORDER.indexOf(s);
const worst = (all: Standing[], none: Standing = "idle"): Standing => (all.length ? all.reduce((a, b) => (rank(b) < rank(a) ? b : a)) : none);

/** What a pane tells of itself, as the view reads it from the fleet. */
export interface OvPane {
  /** `host:pane`. */
  key: string;
  host: string;
  /** Its machine's id (a daemon's), when the page knows it. */
  machine?: string;
  id: number;
  /** Its session on the machine. */
  session: { id: number; name: string } | null;
  /** The project its session is in (`prj_…`). */
  project?: string | null;
  on_issue?: OnIssue | null;
  /** Its title as people read it: the Claude Code conversation's name in a terminal or an agent block, which is how a session is named to the hub and to a board's lead. */
  title: string;
  /** Its agent and title, or what it runs: `Claude · #983 cloud half`. */
  name: string;
  /** Whether it's an agent (it leads its session's tile). */
  agent: boolean;
  /** Whose it is when it isn't the reader's: their name, or "" where
   * that isn't known (#1392). Absent: the reader's. A pane of someone
   * else's that needs them stands `waits`, not `needs`. */
  owner?: string;
  standing: Standing;
  /** When it last printed (ms), for the order of what's quiet. */
  last: number;
}

/** A session as a tile shows it: its name, where it runs and how it
 * stands; `lead` is the pane it opens (none for a session the machines
 * don't show this page, such as one in a cloud workspace). */
export interface OvSession {
  key: string;
  name: string;
  /** Where it runs: a machine's name, or Arugula cloud. */
  where: string;
  standing: Standing;
  /** The pane a click opens, and every pane of it here. */
  lead?: OvPane;
  panes: OvPane[];
  /** A cloud workspace's session, by the hub's id. */
  hosted?: string;
  /** The hub's id for it (`ses_…`), where the hub lists it: the Work
   * level's address takes it as well as the tile's own key (#1380). */
  hub?: string;
  /** Its branch, where the hub says (a session the machines don't show here). */
  branch?: string | null;
  /** Whom it waits on, when that's someone else (`waits`): their name, or "" where that isn't known. */
  asks?: string;
}

export interface OvCard {
  board: string;
  row: BoardRow;
  sessions: OvSession[];
  standing: Standing;
  /** Whom it waits on, when that's someone else (`waits`): its question's
   * person, else its first session's that waits; "" where that isn't known. */
  asks?: string;
}

export interface OvBoard {
  id: string;
  title: string;
  doing: OvCard[];
  review: OvCard[];
  /** How many cards are in Next; how many with an issue or a PR are in Done (`proven` of them), of `total`. */
  next: number;
  done: number;
  proven: number;
  total: number;
  standing: Standing;
  /** The session that leads it (#414), when one does. */
  lead?: OvLead;
}

/** A board's lead: its machine (id and name) and its session's name there;
 * `standing` is how it stands where the machines or the hub tell of it
 * (null: neither does). `peek` is that session as its chip opens it
 * (#1290): its panes here, else the hub's, else only where it runs. */
export interface OvLead {
  machine: string;
  machineName: string;
  session: string;
  standing: Standing | null;
  peek: OvSession;
}

export interface Overview {
  /** The open boards, the most urgent first, those with nothing in flight last. */
  boards: OvBoard[];
  /** Not on a card: the project's sessions that work on none and lead no board. */
  loose: OvSession[];
  /** Where each of the project's panes is: its card, or none. */
  cardOf: Map<string, { board: string; row: BoardRow } | null>;
}

/** Where a card points, for the hub's group: its `iss_…`. */
const cardId = (r: BoardRow): string | null => (r.pointer.kind === "issue" ? r.pointer.id : null);

/** A card's number as an agent was started on it (`#12`), for a row whose item is a native issue. */
const numberOf = (issue: string): number | null => {
  const m = /^#(\d+)$/.exec(issue.trim());
  return m ? Number(m[1]) : null;
};

/** The Overview for `project`: its open boards' views, the panes the
 * machines report (every machine's; only the project's are kept) and the
 * hub's sessions by card, null until it answers (or where it can't).
 * `ownerOf` says whose a machine is, for a session the hub names that no
 * pane here shows: as `OvPane.owner`, undefined for the reader's. Without
 * it, or for a cloud workspace's session, a session is the reader's. */
export function overviewOf(project: string, views: BoardView[], panes: OvPane[], byCard: SessionsByCard | null, hostOf: (machine: string) => string | undefined, ownerOf?: (machine: string) => string | undefined): Overview {
  const open = views.filter((v) => !v.board.closed_at);
  const rows = new Map<string, { board: string; row: BoardRow }>();
  const byIssue = new Map<string, { board: string; row: BoardRow }>();
  const byNumber = new Map<number, { board: string; row: BoardRow }>();
  for (const v of open)
    for (const z of v.zones)
      for (const r of z.rows) {
        const at = { board: v.board.id, row: r };
        rows.set(`${v.board.id}/${r.id}`, at);
        const id = cardId(r);
        // A card on two boards: the first board's (the board list's order).
        if (id && !byIssue.has(id)) byIssue.set(id, at);
        if (r.pointer.kind === "issue" && r.live?.kind === "native" && !byNumber.has(r.live.number)) byNumber.set(r.live.number, at);
      }
  const byBlock = new Map(panes.map((p) => [`${p.host}:${p.id}`, p.key]));
  const cardOf = new Map<string, { board: string; row: BoardRow } | null>();
  // 1. An agent's own: the board and row its `on_issue` names, else the card by its number.
  for (const p of panes) {
    const on = p.on_issue;
    if (!on) continue;
    const named = on.boards?.map((b) => rows.get(`${b.board}/${b.row}`)).find(Boolean);
    const n = numberOf(on.issue);
    const at = named ?? (on.issue.startsWith("iss_") ? byIssue.get(on.issue) : n !== null && (!on.project || on.project === project) ? byNumber.get(n) : undefined);
    if (at) cardOf.set(p.key, at);
  }
  // 2. A row's attachment of its block says so too, and wins.
  for (const v of open)
    for (const z of v.zones)
      for (const r of z.rows)
        for (const a of r.attachments) {
          if (a.role !== "work" || a.released || a.pointer.kind !== "block") continue;
          const host = hostOf(a.pointer.machine);
          const key = host && byBlock.get(`${host}:${a.pointer.pane.replace(/^%/, "")}`);
          if (key) cardOf.set(key, { board: v.board.id, row: r });
        }
  // 3. The hub's sessions by card: the panes it lists, for those not placed yet.
  const listed = new Set<string>();
  // A pane's session as the hub names it; an agent's id is its pane, not a session's.
  const hubOf = new Map<string, string>();
  const unseen: { card: { board: string; row: BoardRow } | null; s: ProjectSession }[] = [];
  for (const g of byCard?.groups ?? []) {
    const at = g.card ? (byIssue.get(g.card.id) ?? null) : null;
    for (const s of g.sessions) {
      const keys = panesOf([s]).flatMap(({ machine, pane }) => {
        const host = hostOf(machine);
        const key = host && byBlock.get(`${host}:${pane}`);
        return key ? [key] : [];
      });
      for (const k of keys) {
        listed.add(k);
        if (s.kind !== "agent" && !hubOf.has(k)) hubOf.set(k, s.id);
        if (!cardOf.has(k)) cardOf.set(k, at);
      }
      if (!keys.length) unseen.push({ card: at, s });
    }
  }
  // 4. A session's card is its panes' card: a pane with none takes its session's.
  const sessionOf = (p: OvPane) => (p.session ? `${p.host}#${p.session.id}` : p.key);
  const sessionCard = new Map<string, { board: string; row: BoardRow }>();
  for (const p of panes) {
    const at = cardOf.get(p.key);
    if (at && !sessionCard.has(sessionOf(p))) sessionCard.set(sessionOf(p), at);
  }
  // Only the project's panes: its sessions', or those on its cards (or in a session on one) or in its list.
  const mine = panes.filter((p) => p.project === project || p.on_issue?.project === project || !!cardOf.get(p.key) || sessionCard.has(sessionOf(p)) || listed.has(p.key));
  for (const p of mine) if (!cardOf.get(p.key)) cardOf.set(p.key, sessionCard.get(sessionOf(p)) ?? null);

  // The sessions: one tile per session per card, holding its panes on that card.
  const tiles = new Map<string, { at: { board: string; row: BoardRow } | null; panes: OvPane[] }>();
  for (const p of mine) {
    const at = cardOf.get(p.key) ?? null;
    const k = `${at ? `${at.board}/${at.row.id}` : "-"}|${sessionOf(p)}`;
    const t = tiles.get(k);
    if (t) t.panes.push(p);
    else tiles.set(k, { at, panes: [p] });
  }
  const onCard = new Map<string, OvSession[]>();
  const loose: OvSession[] = [];
  const put = (at: { board: string; row: BoardRow } | null, s: OvSession) => {
    if (!at) return void loose.push(s);
    const k = `${at.board}/${at.row.id}`;
    onCard.set(k, [...(onCard.get(k) ?? []), s]);
  };
  for (const [k, t] of tiles) {
    const lead = leadOf(t.panes);
    // Named by its agent where it has one, though the pane it opens is the one asking.
    const agent = t.panes.find((p) => p.agent);
    const name = agent ? agent.name : lead.session && t.panes.length > 1 ? lead.session.name : lead.name;
    const hub = t.panes.map((p) => hubOf.get(p.key)).find(Boolean);
    const standing = worst(t.panes.map(paneStanding));
    // Whom it waits on: the first of its panes that waits.
    const asks = standing === "waits" ? t.panes.find((p) => paneStanding(p) === "waits")!.owner : undefined;
    put(t.at, { key: k, name, where: lead.host, standing, lead, panes: t.panes, ...(hub ? { hub } : {}), ...(asks !== undefined ? { asks } : {}) });
  }
  // The hub's sessions the machines don't show here: named, where they run, no pane.
  for (const { card, s } of unseen) {
    const where = s.on.kind === "hosted" ? "Arugula cloud" : s.on.name;
    const owner = s.on.kind === "machine" && ownerOf ? ownerOf(s.on.id) : undefined;
    const said = hubStanding(s);
    const standing = said === "needs" && owner !== undefined ? "waits" : said;
    put(card, { key: `${s.kind}:${s.on.kind === "machine" ? `${s.on.id}:` : ""}${s.id}`, name: s.name, where, standing, panes: [], branch: s.branch, ...(s.kind === "hosted" ? { hosted: s.id } : {}), ...(s.kind !== "agent" ? { hub: s.id } : {}), ...(standing === "waits" ? { asks: owner } : {}) });
  }

  const boards: OvBoard[] = [];
  for (const v of open) {
    const all = v.zones.flatMap((z) => z.rows);
    const work = all.filter((r) => r.pointer.kind === "issue" || r.pointer.kind === "forge_item");
    const cards = (lane: "doing" | "review") =>
      sortCards(
        all
          .filter((r) => r.lane === lane)
          .map((r): OvCard => {
            const sessions = sortSessions(onCard.get(`${v.board.id}/${r.id}`) ?? []);
            const standing = worst([...sessions.map((s) => s.standing), ...rowSays(r)]);
            // Whom it waits on: its question's person first, else its first session's that waits.
            const asks = standing !== "waits" ? undefined : r.ask?.who && !r.ask.mine ? r.ask.who_name : sessions.find((s) => s.standing === "waits")?.asks;
            return { board: v.board.id, row: r, sessions, standing, ...(asks !== undefined ? { asks } : {}) };
          }),
      );
    const doing = cards("doing");
    const review = cards("review");
    boards.push({
      id: v.board.id,
      title: v.board.title,
      doing,
      review,
      next: all.filter((r) => r.lane === "next").length,
      // Done and the total count what the Boards page's bar does: the rows with an issue or a PR.
      done: work.filter((r) => r.lane === "done").length,
      proven: work.filter((r) => r.lane === "done" && r.proof).length,
      total: work.length,
      standing: worst([...doing, ...review].map((c) => c.standing)),
      ...(v.board.lead ? { lead: boardLead(v.board.lead, panes, byCard, hostOf) } : {}),
    });
  }
  const still = (b: OvBoard) => Number(!b.doing.length && !b.review.length);
  boards.sort((a, b) => still(a) - still(b) || rank(a.standing) - rank(b.standing) || a.title.localeCompare(b.title));
  // A session that leads an open board is shown as its lead, not as one on no card (#1508): by its panes, or by the hub's session the lead stands as.
  const leads = boards.flatMap((b) => (b.lead ? [b.lead.peek] : []));
  const leading = (s: OvSession) => leads.some((l) => l.key === s.key || l.panes.some((p) => s.panes.some((q) => q.key === p.key)));
  return { boards, loose: sortSessions(loose.filter((s) => !leading(s))), cardOf };
}

/** A board's lead as the row names it, or a staff room's (#1342): how it
 * stands from its session's panes on its machine, else from the hub's
 * sessions by card. */
export function boardLead(
  l: Pick<BoardLead, "machine" | "machine_name" | "session" | "pane">,
  panes: OvPane[],
  byCard: SessionsByCard | null,
  hostOf: (machine: string) => string | undefined,
  projectName?: (id: string) => string | undefined,
): OvLead {
  // A chief of staff's session goes by its project's id, its block by its project's name (Arugula #1457): shown by that, and its pane found by either.
  const shown = sessionLabel(l.session, projectName);
  // A pane whose machine has no id here (a daemon's own page) is matched by its name.
  const on = (p: OvPane) => (p.machine ? p.machine === l.machine : !!l.machine_name && p.host === l.machine_name);
  // Two projects of one name have chiefs of staff of one title: of those, its project's, where a pane says it.
  const chief = chiefProject(l.session);
  const titles = panes.filter((p) => on(p) && p.session && (p.title === l.session || p.title === shown));
  const ours = chief ? titles.filter((p) => p.project === chief) : [];
  const called = ours.length ? ours : titles;
  // The daemon names a lead by its Claude Code conversation's title (#429), not by the session its pane sits in: a terminal's session is the tab group's own name. So a session by name, else the session of the pane with that title.
  const named = panes.filter((p) => on(p) && p.session?.name === l.session);
  const titled = new Set(called.map((p) => `${p.host}#${p.session!.id}`));
  const guessed = named.length ? named : panes.filter((p) => p.session && titled.has(`${p.host}#${p.session.id}`));
  // A lead that carries its pane (#1368) is that pane alone, standing and all: a terminal's session is its whole tab group, and the rest of it isn't the lead. Only a lead from an older daemon, or one whose pane isn't in this fleet, is guessed from the name or title, by its session.
  const pinned = l.pane === undefined ? undefined : panes.find((p) => on(p) && p.id === l.pane);
  const mine = pinned ? [pinned] : guessed;
  const hub = byCard?.groups.flatMap((g) => g.sessions).find((s) => s.on.kind === "machine" && s.on.id === l.machine && s.name === l.session);
  const standing = mine.length ? worst(mine.map(paneStanding)) : hub ? hubStanding(hub) : null;
  const machineName = l.machine_name || hostOf(l.machine) || "";
  // The hub's id for its session; an agent's id is its pane, not a session's.
  const hubId = hub && hub.kind !== "agent" ? { hub: hub.id } : {};
  // Keyed as its tile off a card is, so its peek is the same tab as one opened from its row: by the hub's id where it names the session, except a lead that is one pane (#1390).
  const lead = pinned ?? (mine.length ? leadOf(mine) : undefined);
  const peek: OvSession = lead
    ? { key: pinned ? `-|${lead.host}:${lead.id}` : `-|${lead.host}#${lead.session!.id}`, name: shown, where: lead.host, standing: standing!, lead, panes: mine, ...(pinned ? {} : hubId) }
    : hub
      ? { key: `${hub.kind}:${l.machine}:${hub.id}`, name: shown, where: hub.on.kind === "machine" ? hub.on.name : machineName, standing: standing!, panes: [], branch: hub.branch, ...hubId }
      : { key: `lead:${l.machine}:${l.session}`, name: shown, where: machineName || "its machine", standing: "idle", panes: [] };
  return { machine: l.machine, machineName, session: l.session, standing, peek };
}

/** Where a lead runs and whether it's awake (#1342), for its tab's head:
 * `awake` while a turn runs; `idle` on a machine control has online (a
 * message wakes it); `asleep` on one that's offline, or a Cloud
 * workspace's paused box (a message waits); `unknown` where neither its
 * panes nor the hub tell; `none` with no lead. `host` is its machine in
 * the page's fleet, which names it (a Cloud workspace by its own name). */
export type LeadAwake = { state: "none" } | { state: "awake" | "idle" | "asleep" | "unknown"; machine: string };

export function leadAwake(lead: Pick<OvLead, "machineName" | "standing"> | null, host: { name: string; status?: string; state: string } | undefined): LeadAwake {
  if (!lead) return { state: "none" };
  const machine = host?.name || lead.machineName;
  // A machine control no longer names has left: nothing hears it.
  if (!machine) return { state: "asleep", machine: "its machine" };
  const offline = host ? host.status === "offline" || host.state === "asleep" || (!host.status && host.state === "offline") : false;
  if (offline) return { state: "asleep", machine };
  if (lead.standing === "working") return { state: "awake", machine };
  // Its panes or the hub tell how it stands, or its machine is connected here and shows no turn running.
  if (lead.standing !== null || host?.state === "connected" || host?.status === "online") return { state: "idle", machine };
  return { state: "unknown", machine };
}

/** How a pane stands for the reader: one that needs someone else waits on them (#1392). */
const paneStanding = (p: OvPane): Standing => (p.standing === "needs" && p.owner !== undefined ? "waits" : p.standing);

/** What a row says of itself, besides its sessions: its question (`ask.who`)
 * for you, or for someone else, its stalled mark, its PRs' checks and
 * review (the row's own PR, and the PRs attached as its work), and its lane. */
function rowSays(r: BoardRow): Standing[] {
  const says: Standing[] = [];
  if (r.ask?.who) says.push(r.ask.mine ? "asks" : "waits");
  if (r.marks?.some((m) => m.kind === "stalled")) says.push("quiet");
  const prs = [r.live, ...r.attachments.filter((a) => a.role === "work" && !a.released).map((a) => a.live)].filter((l) => l?.kind === "pr");
  for (const l of prs) {
    if (l!.state === "checks_failed") says.push("checks_failing");
    else if (l!.state === "checks_running") says.push("checks_running");
    else if (l!.state === "open") says.push("review");
  }
  if (r.lane === "review") says.push("review");
  return says;
}

/** A board's cards in flight as its card lists them: Doing and Review
 * together, the worst first, up to `max`; and how many more there are. */
export function inFlight(b: Pick<OvBoard, "doing" | "review">, max = 4): { shown: OvCard[]; more: number } {
  const all = sortCards([...b.doing, ...b.review]);
  return { shown: all.slice(0, max), more: Math.max(0, all.length - max) };
}

/** The pane a session's tile opens: one that needs you or someone, else its agent, else its first. */
function leadOf(panes: OvPane[]): OvPane {
  return panes.find((p) => p.standing === "needs") ?? panes.find((p) => p.agent) ?? panes[0];
}

/** The word a ticket says on a board's level: it asks you (its question,
 * or a session of yours on it waiting); it asks someone else, by name
 * (#1392); else its lane's. */
export function ticketWord(r: Pick<BoardRow, "ask" | "lane">, c?: Pick<OvCard, "standing" | "asks">): { word: string; asks: boolean } {
  if (r.ask?.mine || c?.standing === "asks" || c?.standing === "needs") return { word: "asks you", asks: true };
  if (r.ask?.who && !r.ask.mine) return { word: `asks ${r.ask.who_name || "someone"}`, asks: false };
  if (c?.standing === "waits") return { word: wordOf(c), asks: false };
  return { word: r.lane === "doing" ? "working" : r.lane === "review" ? "in review" : r.lane === "next" ? "next" : "done", asks: false };
}

/** The word a card or a session says: one waiting on someone else names
 * them (#1392), `asks Aaron`, or says `asks someone` where that isn't
 * known; anything else, its standing's word. */
export function wordOf(x: { standing: Standing; asks?: string }): string {
  return x.standing === "waits" ? `asks ${x.asks || "someone"}` : STATE_WORD[x.standing];
}

/** Cards by how they stand (`STANDING_ORDER`), the worst first; then the one that moved last. */
export function sortCards(cards: OvCard[]): OvCard[] {
  return [...cards].sort((a, b) => rank(a.standing) - rank(b.standing) || b.row.updated_at - a.row.updated_at);
}

function sortSessions(s: OvSession[]): OvSession[] {
  return [...s].sort((a, b) => rank(a.standing) - rank(b.standing) || (b.lead?.last ?? 0) - (a.lead?.last ?? 0) || a.name.localeCompare(b.name));
}

/** What's going on on a board, in words, for its row: how many of its
 * cards in flight stand each way, the worst first (`1 needs you · 2
 * working`); cards nothing says of, as how many are in flight; nothing in
 * flight, so. */
export function boardSays(b: Pick<OvBoard, "doing" | "review">): { standing: Standing; words: string }[] {
  const all = [...b.doing, ...b.review];
  if (!all.length) return [{ standing: "idle", words: "nothing in flight" }];
  const says: { standing: Standing; words: string }[] = STANDING_ORDER.filter((s) => s !== "idle")
    .map((standing) => ({ standing, n: all.filter((c) => c.standing === standing).length }))
    .filter((x) => x.n > 0)
    .map(({ standing, n }) => ({ standing, words: `${n} ${STATE_WORD[standing]}` }));
  const idle = all.filter((c) => c.standing === "idle").length;
  if (idle) says.push({ standing: "idle", words: says.length ? `${idle} more in flight` : `${idle} in flight` });
  return says;
}

/** Whether a board asks something of the reader: a card in flight on it needs them, or asks them. Its row opens by itself. */
export function asksYou(b: Pick<OvBoard, "standing">): boolean {
  return b.standing === "needs" || b.standing === "asks";
}

/** One owner's asks in Needs you (#1333): yours, or a person's or a team's by name. */
export interface AskGroup<T> {
  /** `me`, or the name it's grouped under. */
  key: string;
  /** Null for yours. */
  name: string | null;
  items: T[];
}

/** Needs you's asks by owner (#1333): yours first, then one group per
 * other name, the one with more asks first and then by name; each keeps
 * the order it came in (`byNeedsYou`'s). `ownerOf` says whose an ask is:
 * null for yours, else the name to group it under. */
export function groupByOwner<T>(items: T[], ownerOf: (t: T) => string | null): AskGroup<T>[] {
  const groups = new Map<string, AskGroup<T>>();
  for (const t of items) {
    const name = ownerOf(t);
    const key = name === null ? "me" : `name:${name}`;
    const g = groups.get(key) ?? groups.set(key, { key, name, items: [] }).get(key)!;
    g.items.push(t);
  }
  const mine = groups.get("me");
  const others = [...groups.values()].filter((g) => g.name !== null).sort((a, b) => b.items.length - a.items.length || a.name!.localeCompare(b.name!));
  return mine ? [mine, ...others] : others;
}

/** The bar's line: `7 cards in flight · 4 sessions working · 2 need you`. */
export function statsLine(o: Pick<Overview, "boards" | "loose">, needs: number): string {
  const cards = o.boards.reduce((n, b) => n + b.doing.length + b.review.length, 0);
  const sessions = [...o.boards.flatMap((b) => [...b.doing, ...b.review].flatMap((c) => c.sessions)), ...o.loose];
  const working = sessions.filter((s) => s.standing === "working").length;
  return `${cards} ${cards === 1 ? "card" : "cards"} in flight · ${working} ${working === 1 ? "session" : "sessions"} working · ${needs} ${needs === 1 ? "needs" : "need"} you`;
}

/** Whose conversations Your work lists on a project's Overview (Arugula
 * #1400): on the reader's personal project, the machines they own, by name;
 * never a teammate's, a team's own box or a hosted one, and on any other
 * project none. */
export function yourWorkMachines(personal: boolean, hosts: { name: string; owner?: string; teamBox?: boolean; hosted?: boolean }[]): string[] {
  return personal ? hosts.filter((h) => !h.owner && !h.teamBox && !h.hosted).map((h) => h.name) : [];
}

/** A machine the chief of staff may start on. */
export interface StaffSpot {
  /** Its host's name in the fleet: what the request goes to. */
  name: string;
  /** Control's id for it, which a grant names; none on a daemon's own page. */
  id?: string;
  /** A Cloud workspace of the project's. */
  cloud: boolean;
  /** The machine this page is on. */
  here: boolean;
}

/** Where the chief of staff can start: `can`, here first, then the
 * reader's machines by name, then the project's Cloud workspaces; the
 * default (`pick`), here if it can, else the first of the reader's own
 * machines (never a Cloud workspace); and `cannot`, the reader's machines
 * left out and why (Arugula #1341). `hostedIn` is the project of a Cloud
 * workspace's machine; `features`, what a machine's daemon says it does:
 * an older one doesn't say `chief_of_staff`. */
export function staffSpots(
  hosts: { name: string; id?: string; state: string; status?: string; owner?: string; teamBox?: boolean; hosted?: boolean }[],
  o: { project: string; here: string | null; features: (name: string) => { chief_of_staff?: boolean } | null | undefined; hostedIn: (id: string | undefined) => string | undefined },
): { can: StaffSpot[]; pick: string | null; cannot: { name: string; why: string }[] } {
  const can: StaffSpot[] = [];
  const cannot: { name: string; why: string }[] = [];
  for (const h of hosts) {
    // Yours: not a teammate's, not a team's own box; a Cloud workspace of this project's.
    const cloud = !!h.hosted;
    if (cloud ? o.hostedIn(h.id) !== o.project : h.owner || h.teamBox) continue;
    // Started over this page's connection to it, so only one it reaches now.
    const away = h.state === "asleep" ? "asleep" : h.status === "offline" || h.state === "offline" ? "offline" : h.state !== "connected" ? "not reached from here" : null;
    const why = away ?? (o.features(h.name)?.chief_of_staff !== true ? "needs an update" : null);
    if (why) {
      if (!cloud) cannot.push({ name: h.name, why });
      continue;
    }
    can.push({ name: h.name, id: h.id, cloud, here: !!o.here && h.name === o.here });
  }
  can.sort((a, b) => Number(b.here) - Number(a.here) || Number(a.cloud) - Number(b.cloud) || a.name.localeCompare(b.name));
  cannot.sort((a, b) => a.name.localeCompare(b.name));
  const pick = can.find((s) => s.here)?.name ?? can.find((s) => !s.cloud)?.name ?? null;
  return { can, pick, cannot };
}
