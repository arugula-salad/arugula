// What a move takes, worked out from what's picked (Arugula #971's spike).
// This stands in for the split planner (#968), which answers the same
// thing from the database without changing anything: the screen draws a
// `Plan` and never works one out itself. The rules are the Doc's (#968):
// work goes with its repo; picking a board picks its repo; a board with no
// repo moves on its own; an issue with no repo goes with its board; a Doc
// linked to a moving board comes along; a machine's Docs move from that
// machine.

import type { Board, Doc, Issue, Owner, Project, Repo, Source } from "./fixture";

/** What the person has picked. Boards here are the ones with no repo (a
 * repo's boards come with it); issues, the loose ones (no repo, no board). */
export interface Picks {
  /** "new", or an existing project's id. */
  target: string;
  name: string;
  owner: string;
  repos: Set<string>;
  boards: Set<string>;
  issues: Set<string>;
  docs: Set<string>;
}

export const noPicks = (): Picks => ({ target: "new", name: "", owner: "personal", repos: new Set(), boards: new Set(), issues: new Set(), docs: new Set() });

/** An issue that moves, with the number it gets. */
export interface Renumbered {
  issue: Issue;
  to: number;
}

/** Something the person has to deal with before moving, in plain words. */
export interface Note {
  key: string;
  text: string;
}

export interface Plan {
  /** The target's name, for the button. */
  to: string;
  repos: Repo[];
  boards: Board[];
  issues: Renumbered[];
  /** GitHub's issues and PRs on the moving repos: they follow, unchanged. */
  github: number;
  /** Sessions whose project label follows a moving repo. */
  sessions: number;
  docs: Doc[];
  notes: Note[];
  /** Nothing picked yet. */
  empty: boolean;
}

/** Why a board, issue or Doc is in without its own pick: "with owner/repo". */
export function why(s: Source, p: Picks, item: { repo?: string; board?: string }): string | null {
  if (item.repo && p.repos.has(item.repo)) return `with ${s.repos.find((r) => r.id === item.repo)?.name}`;
  if (item.board && boardMoves(s, p, item.board)) return "with its board";
  return null;
}

export function boardMoves(s: Source, p: Picks, id: string): boolean {
  const b = s.boards.find((x) => x.id === id);
  return !!b && (b.repo ? p.repos.has(b.repo) : p.boards.has(b.id));
}

/** A Doc this screen can pick: on the platform, or in the library of the
 * machine the screen is open on (`here`), which then moves it. */
export const pickable = (d: Doc, here: string | null) => !d.machine || d.machine === here;

export function plan(s: Source, p: Picks, here: string | null, targets: Project[], owners: Owner[]): Plan {
  const existing = targets.find((t) => t.id === p.target);
  const to = existing?.name ?? (p.name.trim() || "the new project");
  const repos = s.repos.filter((r) => p.repos.has(r.id));
  const boards = s.boards.filter((b) => boardMoves(s, p, b.id));
  const start = existing?.next ?? 1;
  const issues = s.issues
    .filter((i) => (i.repo ? p.repos.has(i.repo) : i.board ? boardMoves(s, p, i.board) : p.issues.has(i.id)))
    .sort((a, b) => a.n - b.n)
    .map((issue, k) => ({ issue, to: start + k }));
  const docs = s.docs.filter((d) => pickable(d, here) && (p.docs.has(d.id) || (d.board !== undefined && boardMoves(s, p, d.board))));
  const github = repos.reduce((n, r) => n + r.github, 0);
  const sessions = repos.reduce((n, r) => n + r.sessions, 0);
  const empty = !repos.length && !boards.length && !issues.length && !docs.length;

  // Only what the person has to deal with: who will now see it.
  const notes: Note[] = [];
  const owner = existing ? existing.owner : owners.find((o) => o.key === p.owner)?.name;
  if (!empty && owner !== s.project.owner) {
    const people = owners.find((o) => o.name === owner)?.people ?? `everyone in ${owner}`;
    notes.push({ key: "people", text: `${people[0].toUpperCase()}${people.slice(1)} will see what moves.` });
  }

  return { to, repos, boards, issues, github, sessions, docs, notes, empty };
}

/** Case-blind: does `title` hold `q`? */
export const matches = (title: string, q: string) => title.toLowerCase().includes(q.trim().toLowerCase());
