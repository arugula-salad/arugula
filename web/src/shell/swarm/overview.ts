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

/** How a card or a session stands, most urgent first: a session waits on
 * you, the row's question asks you, a session failed, its PR's checks fail,
 * it's quiet, it's working, its PR's checks run, it's in review; or nothing
 * says (`idle`). A session only ever needs you, failed, is quiet or works. */
export type Standing = "needs" | "asks" | "failed" | "checks_failing" | "quiet" | "working" | "checks_running" | "review" | "idle";
export const STANDING_ORDER: Standing[] = ["needs", "asks", "failed", "checks_failing", "quiet", "working", "checks_running", "review", "idle"];
/** The one word a row says of how it stands; nothing for `idle`. */
export const STATE_WORD: Record<Standing, string> = {
  needs: "needs you",
  asks: "asks you",
  failed: "failed",
  checks_failing: "checks failing",
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
  /** Its agent and title, or what it runs: `Claude · #983 cloud half`. */
  name: string;
  /** Whether it's an agent (it leads its session's tile). */
  agent: boolean;
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
  /** Its branch, where the hub says (a session the machines don't show here). */
  branch?: string | null;
}

export interface OvCard {
  board: string;
  row: BoardRow;
  sessions: OvSession[];
  standing: Standing;
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
  /** Not on a card: the project's sessions that work on none. */
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
 * hub's sessions by card, null until it answers (or where it can't). */
export function overviewOf(project: string, views: BoardView[], panes: OvPane[], byCard: SessionsByCard | null, hostOf: (machine: string) => string | undefined): Overview {
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
    put(t.at, { key: k, name, where: lead.host, standing: worst(t.panes.map((p) => p.standing)), lead, panes: t.panes });
  }
  // The hub's sessions the machines don't show here: named, where they run, no pane.
  for (const { card, s } of unseen) {
    const where = s.on.kind === "hosted" ? "Arugula cloud" : s.on.name;
    put(card, { key: `${s.kind}:${s.on.kind === "machine" ? `${s.on.id}:` : ""}${s.id}`, name: s.name, where, standing: hubStanding(s), panes: [], branch: s.branch, ...(s.kind === "hosted" ? { hosted: s.id } : {}) });
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
            return { board: v.board.id, row: r, sessions, standing: worst([...sessions.map((s) => s.standing), ...rowSays(r)]) };
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
  return { boards, loose: sortSessions(loose), cardOf };
}

/** A board's lead as the row names it: how it stands from its session's
 * panes on its machine, else from the hub's sessions by card. */
function boardLead(l: BoardLead, panes: OvPane[], byCard: SessionsByCard | null, hostOf: (machine: string) => string | undefined): OvLead {
  // A pane whose machine has no id here (a daemon's own page) is matched by its name.
  const on = (p: OvPane) => (p.machine ? p.machine === l.machine : !!l.machine_name && p.host === l.machine_name);
  const mine = panes.filter((p) => on(p) && p.session?.name === l.session);
  const hub = byCard?.groups.flatMap((g) => g.sessions).find((s) => s.on.kind === "machine" && s.on.id === l.machine && s.name === l.session);
  const standing = mine.length ? worst(mine.map((p) => p.standing)) : hub ? hubStanding(hub) : null;
  const machineName = l.machine_name || hostOf(l.machine) || "";
  // Keyed as its tile off a card is, so its peek is the same tab as one opened from its row.
  const lead = mine.length ? leadOf(mine) : undefined;
  const peek: OvSession = lead
    ? { key: `-|${lead.host}#${lead.session!.id}`, name: l.session, where: lead.host, standing: standing!, lead, panes: mine }
    : hub
      ? { key: `${hub.kind}:${l.machine}:${hub.id}`, name: l.session, where: hub.on.kind === "machine" ? hub.on.name : machineName, standing: standing!, panes: [], branch: hub.branch }
      : { key: `lead:${l.machine}:${l.session}`, name: l.session, where: machineName || "its machine", standing: "idle", panes: [] };
  return { machine: l.machine, machineName, session: l.session, standing, peek };
}

/** What a row says of itself, besides its sessions: its question for you
 * (`ask.who`, the reader's), its stalled mark, its PRs' checks and review
 * (the row's own PR, and the PRs attached as its work), and its lane. */
function rowSays(r: BoardRow): Standing[] {
  const says: Standing[] = [];
  if (r.ask?.who && r.ask.mine) says.push("asks");
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

/** The pane a session's tile opens: one that needs you, else its agent, else its first. */
function leadOf(panes: OvPane[]): OvPane {
  return panes.find((p) => p.standing === "needs") ?? panes.find((p) => p.agent) ?? panes[0];
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

/** The bar's line: `7 cards in flight · 4 sessions working · 2 need you`. */
export function statsLine(o: Pick<Overview, "boards" | "loose">, needs: number): string {
  const cards = o.boards.reduce((n, b) => n + b.doing.length + b.review.length, 0);
  const sessions = [...o.boards.flatMap((b) => [...b.doing, ...b.review].flatMap((c) => c.sessions)), ...o.loose];
  const working = sessions.filter((s) => s.standing === "working").length;
  return `${cards} ${cards === 1 ? "card" : "cards"} in flight · ${working} ${working === 1 ? "session" : "sessions"} working · ${needs} ${needs === 1 ? "needs" : "need"} you`;
}
