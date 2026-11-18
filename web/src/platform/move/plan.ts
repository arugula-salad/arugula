// What a move takes (Arugula #971), as the platform's split planner says it
// (`POST /api/projects/{project}/split/plan`, #968), and the move itself
// (`POST /api/projects/{project}/split`, #969), which takes the same request;
// then a project's recent splits (`GET /api/projects/{project}/splits`) and
// their undo (`POST …/splits/{id}/undo`, #972).
// The screen draws the planner's answer and never works one out itself. The
// rules are the planner's: work goes with its repo; picking a board picks
// its repo; a board with no repo moves on its own; an issue with no repo goes
// with its board; a Doc on a moving board comes along; a machine's Docs move
// from that machine; a session a moving repo placed comes with it, and
// another is picked one by one on the reader's own online machines, which
// control then tells to move it (Arugula #1166). The shapes are the platform's (crates/platform/src/split.rs
// in the cloud's repository), which aren't in proto.gen.ts: the cloud is closed.

import type { ProjectOwnerRef } from "../../shared/proto.gen";
import { platformFetch, refusalOf } from "../../shared/maintenance";

/** Where the picks go: a project made for the move, or one that exists. */
export type To = { kind: "new"; name: string; owner: ProjectOwnerRef } | { kind: "project"; id: string };

/** The request, the plan's and the move's. */
export interface Ask {
  to: To;
  /** Linked repos, `owner/name`. */
  repos: string[];
  /** `brd_` ids. */
  boards: string[];
  /** `iss_` ids. */
  issues: string[];
  /** Project Docs, `pdc_` ids. */
  docs: string[];
  /** Machine sessions picked one by one, `machine/session` (Arugula #1166). */
  sessions: string[];
  /** A list's page (the platform's default is 50, at most 200). */
  limit?: number;
  /** Where each list's page starts: a list's `next`. */
  after?: Partial<Record<"repos" | "boards" | "issues" | "docs", number>>;
}

export interface Page<T> {
  count: number;
  items: T[];
  /** The next page's `after`; none on the last. */
  next?: number;
}

/** Why something comes along without its own pick. */
export type With = { kind: "repo"; repo: string } | { kind: "board"; board: string; title: string };

export interface Note {
  /** `people`, `machine_docs:<machine>`, `secrets:<environment>`. */
  key: string;
  text: string;
}

export interface MachineDocs {
  machine: string;
  /** Its name, or "a machine" for one the reader can't reach. */
  name: string;
  docs: { id: string; title: string }[];
}

/** A machine with sessions in the project (Arugula #1166): the reader's
 * own online ones' can be picked; a repo brings those it placed from any. */
export interface SourceMachine {
  id: string;
  name: string;
  online: boolean;
  /** The reader's own: only its owner moves a session by hand. */
  yours: boolean;
  /** Its owner's name, as people read it. */
  owner: string;
  sessions: {
    id: string;
    name: string;
    /** The GitHub repo (`owner/name`) that placed it, which it follows. */
    repo?: string;
    /** Its project was set by hand. */
    by_hand: boolean;
  }[];
}

/** A machine's sessions that move, each with who sees it after. */
export interface MovingSessions {
  machine: string;
  name: string;
  online: boolean;
  sessions: { id: string; name: string; who: string; with?: With }[];
}

export interface Plan {
  from: { id: string; name: string };
  to: {
    /** None for a new project. */
    id?: string;
    name: string;
    owner: { kind: "personal"; account: string } | { kind: "team"; id: string; name: string };
    first_number: number;
  };
  /** Nothing moves. */
  empty: boolean;
  /** What to deal with first: often none. */
  notes: Note[];
  along: {
    repos: Page<{ id: number; name: string }>;
    boards: Page<{ id: string; title: string; rows: number; with?: With }>;
    issues: Page<{ id: string; number: number; to: number; title: string; closed: boolean; with?: With }>;
    github: { issues: number; prs: number; repos: { repo: string; issues: number; prs: number }[] };
    docs: Page<{ id: string; title: string; with?: With }>;
    machine_docs: MachineDocs[];
    workspaces: { id: string; name: string; specs: number; schedules: number; runs: number }[];
    environments: { id: string; name: string; deploys: number }[];
    /** Cloud agent sessions in a moving repo. */
    sessions: number;
    drafts: number;
    /** Machine sessions that move, by machine; none from an older control. */
    machine_sessions?: MovingSessions[];
  };
  /** What there is to pick. */
  source: {
    repos: { id: number; name: string; boards: number; issues: number; github: number; sessions: number }[];
    /** `repo`: the repo it's tied to; picking the board picks it. `docs`: the project Docs on it. */
    boards: { id: string; title: string; repo?: string; rows: number; closed: boolean; docs: string[] }[];
    /** Issues on no repo and no board: picked one by one. */
    issues: { id: string; number: number; title: string; closed: boolean }[];
    /** The project's Docs; `board`, the first board it's on. */
    docs: { id: string; title: string; board?: string }[];
    /** Machines with sessions in the project; none from an older control. */
    machines?: SourceMachine[];
  };
}

/** What the move answers. */
export interface Done {
  /** The split's id (`spl_`). */
  id: string;
  to: { id: string; name: string };
  moved: Record<"repos" | "boards" | "issues" | "docs" | "workspaces" | "environments" | "runs" | "sessions" | "drafts" | "links", number>;
  /** Machine sessions: moved now, waiting for their machine (a repo's, on
   * one that's offline), or refused by it. */
  machine_sessions?: {
    moved: number;
    waiting: { machine: string; name: string; sessions: string[] }[];
    failed: { machine: string; name: string; session: string; why: string }[];
  };
}

/** A refusal: its status, the platform's words, and a 409's `reason`
 * (`changed`, `name_taken`, `already_undone`, `too_old`, `in_the_way`). */
export class Refused extends Error {
  constructor(
    readonly status: number,
    message: string,
    readonly reason?: string,
  ) {
    super(message);
  }
}

/** The most each of the plan's lists asks for at once. */
export const PAGE_MAX = 200;

const base = (project: string) => `/api/projects/${encodeURIComponent(project)}/split`;

async function post<T>(path: string, body?: unknown, signal?: AbortSignal): Promise<T> {
  const r = await platformFetch(path, { method: "POST", credentials: "same-origin", signal, ...(body === undefined ? {} : { headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) }) });
  const v = (await r.json().catch(() => ({}))) as T & { error?: string; reason?: string };
  if (!r.ok) throw new Refused(r.status, refusalOf(r.status, v), v.reason);
  return v;
}

/** What moving `ask` out of `project` takes. Changes nothing. */
export const plan = (project: string, ask: Ask, signal?: AbortSignal) => post<Plan>(`${base(project)}/plan`, ask, signal);

/** Move it. A 409 is "it changed since the plan" (plan again), or a new
 * project's name its owner has already. */
export const move = (project: string, ask: Ask) => post<Done>(base(project), { ...ask, limit: undefined, after: undefined });

/** One of a project's splits of the last 7 days, out of it or into it. */
export interface Split {
  id: string;
  direction: "out" | "in";
  /** The project on the other side ("a project" when the reader can't read it). */
  other: { id: string; name: string };
  moved: Done["moved"];
  moved_at: number;
  undone_at?: number;
  /** Whether the reader may undo it now. It may still be refused. */
  may_undo: boolean;
}

/** What an undo answers. */
export interface Undone {
  id: string;
  /** The project the work went back to. */
  to: { id: string; name: string };
  /** The project it was in was made for the split, and is gone. */
  deleted: boolean;
  moved: Done["moved"];
  /** Each issue's number in the project it was split into, and again now. */
  issues: { id: string; from: number; to: number }[];
}

/** The project's splits of the last 7 days, the latest first. */
export async function splits(project: string): Promise<Split[]> {
  const r = await platformFetch(`${base(project)}s`, { credentials: "same-origin" });
  const v = (await r.json().catch(() => ({}))) as { splits?: Split[]; error?: string; reason?: string };
  if (!r.ok) throw new Refused(r.status, refusalOf(r.status, v), v.reason);
  return v.splits ?? [];
}

/** Undo split `id`, on `project`, the one the work left. */
export const undo = (project: string, id: string) => post<Undone>(`${base(project)}s/${encodeURIComponent(id)}/undo`);

/** A 409 that says plan again, not a name that's taken. */
export const changed = (e: unknown) => e instanceof Refused && e.status === 409 && e.reason === "changed";

/** What the person has picked. Boards are the ones with no repo (a repo's
 * boards come with it); issues, the loose ones; `machineDocs`, a machine's
 * Docs as `machine/doc`, which that machine moves. */
export interface Picks {
  /** "new", or an existing project's id. */
  target: string;
  name: string;
  /** `personal`, or a team's id. */
  owner: string;
  repos: Set<string>;
  boards: Set<string>;
  issues: Set<string>;
  docs: Set<string>;
  machineDocs: Set<string>;
  /** Machine sessions, `machine/session`. */
  sessions: Set<string>;
}

export const noPicks = (owner = "personal"): Picks => ({ target: "new", name: "", owner, repos: new Set(), boards: new Set(), issues: new Set(), docs: new Set(), machineDocs: new Set(), sessions: new Set() });

/** The request for `p`, over the project's `machines`: a session counts
 * as picked while it's one the reader may move and no moving repo brings it. */
export function askOf(p: Picks, machines: SourceMachine[] = []): Ask {
  const to: To = p.target === "new" ? { kind: "new", name: p.name.trim(), owner: p.owner === "personal" ? { kind: "personal" } : { kind: "team", id: p.owner } } : { kind: "project", id: p.target };
  const sessions = machines.flatMap((m) => (m.online && m.yours ? m.sessions.filter((s) => !brought(s, p) && p.sessions.has(`${m.id}/${s.id}`)).map((s) => `${m.id}/${s.id}`) : []));
  return { to, repos: [...p.repos], boards: [...p.boards], issues: [...p.issues], docs: [...p.docs], sessions, limit: PAGE_MAX };
}

/** Whether a repo `p` moves brings session `s`, the one that placed it. */
export const brought = (s: SourceMachine["sessions"][number], p: Picks) => !!s.repo && p.repos.has(s.repo);

/** Who sees the moving sessions, in one line: theirs when it's the same
 * for all ("The Arugula team watches."), else that it's beside each. */
export function whoLine(moving: MovingSessions[]): string {
  const who = [...new Set(moving.flatMap((m) => m.sessions.map((s) => s.who)))];
  if (!who.length) return "";
  return who.length === 1 ? `${who[0].charAt(0).toUpperCase()}${who[0].slice(1)}.` : "Who sees each is beside it.";
}

/** What happened to the machine sessions, after "Moved to X.": how many
 * moved, which wait for their machine, and the first one refused. */
export function sessionsSaid(done: Done): string {
  const m = done.machine_sessions;
  if (!m || (!m.moved && !m.waiting.length && !m.failed.length)) return "";
  const bits = [m.moved ? `${m.moved} ${m.moved === 1 ? "session" : "sessions"} moved now` : ""];
  for (const w of m.waiting) bits.push(`${w.sessions.join(", ")} ${w.sessions.length === 1 ? "moves" : "move"} when ${w.name} is back`);
  const f = m.failed[0];
  if (f) bits.push(`${m.failed.length === 1 ? f.session : `${m.failed.length} sessions`} stayed on ${f.name}: ${f.why}`);
  const said = bits.filter(Boolean).join("; ");
  return `${said.charAt(0).toUpperCase()}${said.slice(1)}.`;
}

/** Why a row is in without its own pick: "with owner/repo", "with its board". */
export function withWords(w: With | undefined): string | null {
  if (!w) return null;
  return w.kind === "repo" ? `with ${w.repo}` : "with its board";
}

/** Case-blind: does `title` hold `q`? */
export const matches = (title: string, q: string) => title.toLowerCase().includes(q.trim().toLowerCase());
