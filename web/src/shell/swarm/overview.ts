// A project's Overview (Arugula inversion, L6): Swarm for one project,
// grouped by board, then the cards in flight (Doing and Review), each card
// one tile holding the sessions on it, and the sessions on no card last.
// Plain functions from the project's boards, the panes the machines report
// and the hub's sessions by card, for the app's view and its test
// (scripts/overview.test.mjs). A pane finds its card as a board's focus
// does (./board-focus.ts): its agent's `on_issue`, or a row's attachment
// of its block; then by the hub's sessions by card, and by its session (a
// session's card is its panes' card).

import type { BoardRow, BoardView, OnIssue } from "../../shared/proto";
import { panesOf, type SessionsByCard, type ProjectSession } from "../../shared/project-sessions.ts";

/** How a card or a session stands, most urgent first: it waits on you, it
 * failed, it's quiet, it's working, or it has no one on it. */
export type Standing = "needs" | "failed" | "quiet" | "working" | "idle";
export const STANDING_ORDER: Standing[] = ["needs", "failed", "quiet", "working", "idle"];
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
  /** How many cards are in Next, and Done, of `total`. */
  next: number;
  done: number;
  total: number;
  standing: Standing;
}

export interface Overview {
  /** The boards with cards in flight, the most urgent first. */
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
  const byKey = new Map(panes.map((p) => [p.key, p]));
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
  // Only the project's panes: its sessions', or those on its cards or in its list.
  const mine = panes.filter((p) => p.project === project || p.on_issue?.project === project || !!cardOf.get(p.key) || listed.has(p.key));
  // 4. A session's card is its panes' card: a pane with none takes its session's.
  const sessionOf = (p: OvPane) => (p.session ? `${p.host}#${p.session.id}` : p.key);
  const sessionCard = new Map<string, { board: string; row: BoardRow }>();
  for (const p of mine) {
    const at = cardOf.get(p.key);
    if (at && !sessionCard.has(sessionOf(p))) sessionCard.set(sessionOf(p), at);
  }
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
    const name = lead.session && !lead.agent && t.panes.length > 1 ? lead.session.name : lead.name;
    put(t.at, { key: k, name, where: lead.host, standing: worst(t.panes.map((p) => p.standing)), lead, panes: t.panes });
  }
  // The hub's sessions the machines don't show here: named, where they run, no pane.
  for (const { card, s } of unseen) {
    const where = s.on.kind === "hosted" ? "Arugula cloud" : s.on.name;
    put(card, { key: `${s.kind}:${s.on.kind === "machine" ? `${s.on.id}:` : ""}${s.id}`, name: s.name, where, standing: hubStanding(s), panes: [], ...(s.kind === "hosted" ? { hosted: s.id } : {}) });
  }

  const boards: OvBoard[] = [];
  for (const v of open) {
    const all = v.zones.flatMap((z) => z.rows);
    const cards = (lane: "doing" | "review") =>
      sortCards(
        all
          .filter((r) => r.lane === lane)
          .map((r): OvCard => {
            const sessions = sortSessions(onCard.get(`${v.board.id}/${r.id}`) ?? []);
            const stalled = r.marks?.some((m) => m.kind === "stalled");
            const own = sessions.map((s) => s.standing);
            return { board: v.board.id, row: r, sessions, standing: worst(stalled ? [...own, "quiet"] : own, sessions.length ? "quiet" : "idle") };
          }),
      );
    const doing = cards("doing");
    const review = cards("review");
    if (!doing.length && !review.length) continue;
    boards.push({
      id: v.board.id,
      title: v.board.title,
      doing,
      review,
      next: all.filter((r) => r.lane === "next").length,
      done: all.filter((r) => r.lane === "done").length,
      total: all.length,
      standing: worst([...doing, ...review].map((c) => c.standing)),
    });
  }
  boards.sort((a, b) => rank(a.standing) - rank(b.standing) || a.title.localeCompare(b.title));
  return { boards, loose: sortSessions(loose), cardOf };
}

/** The pane a session's tile opens: one that needs you, else its agent, else its first. */
function leadOf(panes: OvPane[]): OvPane {
  return panes.find((p) => p.standing === "needs") ?? panes.find((p) => p.agent) ?? panes[0];
}

/** A session the hub names but the machines don't show here, by the state it says. */
function hubStanding(s: ProjectSession): Standing {
  const states = s.kind === "machine" ? s.panes.map((p) => p.state ?? "") : [s.state];
  if (states.some((x) => x === "needs_input" || x === "waiting")) return "needs";
  if (states.some((x) => x === "failed" || x === "error")) return "failed";
  if (states.some((x) => x === "working" || x === "running")) return "working";
  return "quiet";
}

/** Cards that need you first, then failures, then quiet ones, then working ones; then the one that moved last. */
export function sortCards(cards: OvCard[]): OvCard[] {
  return [...cards].sort((a, b) => rank(a.standing) - rank(b.standing) || b.row.updated_at - a.row.updated_at);
}

function sortSessions(s: OvSession[]): OvSession[] {
  return [...s].sort((a, b) => rank(a.standing) - rank(b.standing) || (b.lead?.last ?? 0) - (a.lead?.last ?? 0) || a.name.localeCompare(b.name));
}

/** The board's header line: `2 in flight · 5 next · 3 of 10 done`. */
export function boardLine(b: Pick<OvBoard, "doing" | "review" | "next" | "done" | "total">): string {
  return `${b.doing.length + b.review.length} in flight · ${b.next} next · ${b.done} of ${b.total} done`;
}

/** The bar's line: `7 cards in flight · 4 sessions working · 2 need you`. */
export function statsLine(o: Pick<Overview, "boards" | "loose">, needs: number): string {
  const cards = o.boards.reduce((n, b) => n + b.doing.length + b.review.length, 0);
  const sessions = [...o.boards.flatMap((b) => [...b.doing, ...b.review].flatMap((c) => c.sessions)), ...o.loose];
  const working = sessions.filter((s) => s.standing === "working").length;
  return `${cards} ${cards === 1 ? "card" : "cards"} in flight · ${working} ${working === 1 ? "session" : "sessions"} working · ${needs} ${needs === 1 ? "needs" : "need"} you`;
}
