// What a move takes (Arugula #971), as the platform's split planner says it
// (`POST /api/projects/{project}/split/plan`, #968), and the move itself
// (`POST /api/projects/{project}/split`, #969), which takes the same request.
// The screen draws the planner's answer and never works one out itself. The
// rules are the planner's: work goes with its repo; picking a board picks
// its repo; a board with no repo moves on its own; an issue with no repo goes
// with its board; a Doc on a moving board comes along; a machine's Docs move
// from that machine. The shapes are the platform's (crates/platform/src/split.rs
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
    sessions: number;
    drafts: number;
  };
  /** What there is to pick. */
  source: {
    repos: { id: number; name: string; boards: number; issues: number; github: number; sessions: number }[];
    /** `repo`: the repo it's tied to; picking the board picks it. `docs`: the project Docs on it. */
    boards: { id: string; title: string; repo?: string; rows: number; closed: boolean; docs: string[] }[];
    /** Issues on no repo and no board: picked one by one. */
    issues: { id: string; number: number; title: string; closed: boolean }[];
  };
}

/** What the move answers. */
export interface Done {
  /** The split's id (`spl_`). */
  id: string;
  to: { id: string; name: string };
  moved: Record<"repos" | "boards" | "issues" | "docs" | "workspaces" | "environments" | "runs" | "sessions" | "drafts" | "links", number>;
}

/** A refusal: its status and the platform's words. */
export class Refused extends Error {
  constructor(
    readonly status: number,
    message: string,
  ) {
    super(message);
  }
}

/** The most each of the plan's lists asks for at once. */
export const PAGE_MAX = 200;

const base = (project: string) => `/api/projects/${encodeURIComponent(project)}/split`;

async function post<T>(path: string, body: unknown, signal?: AbortSignal): Promise<T> {
  const r = await platformFetch(path, { method: "POST", credentials: "same-origin", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body), signal });
  const v = (await r.json().catch(() => ({}))) as T & { error?: string; reason?: string };
  if (!r.ok) throw new Refused(r.status, refusalOf(r.status, v));
  return v;
}

/** What moving `ask` out of `project` takes. Changes nothing. */
export const plan = (project: string, ask: Ask, signal?: AbortSignal) => post<Plan>(`${base(project)}/plan`, ask, signal);

/** Move it. A 409 is "it changed since the plan" (plan again), or a new
 * project's name its owner has already. */
export const move = (project: string, ask: Ask) => post<Done>(base(project), { ...ask, limit: undefined, after: undefined });

/** A 409 that says plan again, not a name that's taken. */
export const changed = (e: unknown) => e instanceof Refused && e.status === 409 && !/with that name/.test(e.message);

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
}

export const noPicks = (owner = "personal"): Picks => ({ target: "new", name: "", owner, repos: new Set(), boards: new Set(), issues: new Set(), docs: new Set(), machineDocs: new Set() });

/** The planner wants a new project's name to answer even with nothing
 * picked: this one stands in until the person types theirs. */
const UNNAMED = "New project";

/** The request for `p`. */
export function askOf(p: Picks): Ask {
  const to: To = p.target === "new" ? { kind: "new", name: p.name.trim() || UNNAMED, owner: p.owner === "personal" ? { kind: "personal" } : { kind: "team", id: p.owner } } : { kind: "project", id: p.target };
  return { to, repos: [...p.repos], boards: [...p.boards], issues: [...p.issues], docs: [...p.docs], limit: PAGE_MAX };
}

/** Why a row is in without its own pick: "with owner/repo", "with its board". */
export function withWords(w: With | undefined): string | null {
  if (!w) return null;
  return w.kind === "repo" ? `with ${w.repo}` : "with its board";
}

/** Case-blind: does `title` hold `q`? */
export const matches = (title: string, q: string) => title.toLowerCase().includes(q.trim().toLowerCase());
