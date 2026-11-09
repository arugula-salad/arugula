// The account's grants as the platform page reads them (`GET /api/grants`),
// and what the agent switches (./agents, M197) make of them. Plain
// functions, tested in scripts/agents.test.mjs.

/** A grant, as `GET /api/grants` lists it. */
export interface Grant {
  id: string;
  subject: { kind: string; id: string; name: string | null };
  power: string;
  scope: { kind: "project" | "account"; id: string; name?: string | null };
  live: boolean;
  granted_by?: string;
  revoked_at?: number | null;
  /** Who revoked it (Arugula #683): the giver, a project's owner, the
   * platform, or an account since deleted (no account). Older platforms
   * say nothing. */
  revoked_by?: { account: string | null; name: string | null } | null;
  /** What a live one does on a viewer's (Arugula #683): a write power
   * while its giver only views the project, as a label ("idle", "reads
   * only") and why. Older platforms say nothing. */
  limited?: { label: string; why: string } | null;
}

/** The reader's role in a project, as the page knows it. */
export type Role = "owner" | "editor" | "viewer";

/** The powers the platform refuses to a viewer's machine or token (Arugula
 * #683): nothing of them works for a viewer's agents. A copy of the wire's
 * `VIEWER_REFUSED` (crates/control-wire/src/projects.rs), whose test
 * checks this line is the same list; the switches and the token form
 * leave them out for a viewer rather than ask and be refused. */
export const VIEWER_REFUSED = ["issues.write", "trackers.write", "runs.start", "runs.command", "secrets.write"];

/** `powers` without those a viewer's machine is refused, when `role` is viewer. */
export const forRole = (powers: string[], role?: Role) => (role === "viewer" ? powers.filter((p) => !VIEWER_REFUSED.includes(p)) : powers);

/** Where a switch reaches: a project, or your DMs (`account`). */
export interface AgentScope {
  kind: "project" | "account";
  id?: string;
  name: string;
  /** The reader's role in a project (Arugula #683): a viewer's switches
   * give less. */
  role?: Role;
}

/** What a switch gives over each scope: for a viewer's project (Arugula
 * #683), only what a viewer's machine is given. */
export function powersFor(kind: AgentScope["kind"], role?: Role): string[] {
  return forRole(kind === "project" ? ["threads.read", "threads.post", "issues.write"] : ["threads.read", "threads.post"], role);
}

/** M218: an agent's power to plan a project's deploys and ask for their
 * approval. Its own switch, never given with the conversation ones. */
export const DEPLOYS_PLAN = "deploys.plan";

/** What the runs switch gives over a project (M216): its agents read the
 * project's runs, and start its run specs as yours. */
export const RUN_POWERS = ["runs.read", "runs.start"];

/** What the commands switch gives over a project (M216): its agents start
 * commands and instructions of their own there, as yours. */
export const COMMAND_POWERS = ["runs.command"];

/** aru-121: its agents start agent sessions as you (you're the asker, and
 * you pay), and answer or prompt them. A switch each, never given with the
 * conversation ones. */
export const SESSIONS_START = "sessions.start";
export const SESSIONS_REPLY = "sessions.reply";

/** #359: its agents write to the project's connected Plane project (the
 * connection says whether each write waits for a person). A switch of its
 * own, never given with the conversation ones. */
export const TRACKERS_WRITE = "trackers.write";

/** The boards powers (#293): `boards.write` reads too, so the boards column
 * is one control with three levels, not two switches. */
export const BOARDS_READ = "boards.read";
export const BOARDS_WRITE = "boards.write";

/** #366: "Work on boards", one switch for the agent that runs a board: it
 * adds, attaches and asks on the project's boards, files its issues and
 * writes to its Plane project. The three powers together; each is still its
 * own grant. */
export const WORK_ON_BOARDS = [BOARDS_WRITE, "issues.write", TRACKERS_WRITE];

/** Whether `powers` hold the whole of "Work on boards". */
export const hasWorkOnBoards = (powers: string[]) => WORK_ON_BOARDS.every((p) => powers.includes(p));

/** `powers` with "Work on boards" on, or off: the other powers stay. */
export function withWorkOnBoards(powers: string[], on: boolean): string[] {
  const rest = powers.filter((p) => !WORK_ON_BOARDS.includes(p));
  return on ? [...rest, ...WORK_ON_BOARDS] : rest;
}

/** How much of a project's boards a machine's agents have. */
export type BoardsLevel = "off" | "read" | "write";

/** What a machine's agents have over a project's boards: write if
 * `boards.write` is live (whatever else is), else read if `boards.read` is. */
export function boardsLevel(grants: Grant[], machine: string, project: string): BoardsLevel {
  if (singleGrant(grants, machine, project, BOARDS_WRITE)) return "write";
  return singleGrant(grants, machine, project, BOARDS_READ) ? "read" : "off";
}

/** The names of the machines whose agents can't see a project's boards
 * (#313): the ones with no live `boards.read` or `boards.write` over it. */
export function blindMachines(grants: Grant[], project: string): string[] {
  return machinesOf(grants)
    .filter((m) => boardsLevel(grants, m.id, project) === "off")
    .map((m) => m.name);
}

/** The Boards page's line for those machines (#313): up to three by name,
 * then "<a>, <b> and 2 more". Empty for none. */
export function blindLine(names: string[]): string {
  if (names.length === 0) return "";
  const shown = names.length <= 3 ? names : [...names.slice(0, 2), `${names.length - 2} more`];
  const who = shown.length === 1 ? shown[0] : `${shown.slice(0, -1).join(", ")} and ${shown[shown.length - 1]}`;
  return `Agents on ${who} can't see these boards.`;
}

/** What moving to `to` takes: the powers to give, and the live grants to
 * revoke. Down from write to read takes `boards.write` and keeps (or gives)
 * `boards.read`; off takes both. */
export function boardsMove(grants: Grant[], machine: string, project: string, to: BoardsLevel): { give: string[]; revoke: Grant[] } {
  const have = (power: string) => singleGrant(grants, machine, project, power);
  const revoke = (powers: string[]) => powers.flatMap((p) => have(p) ?? []);
  if (to === "off") return { give: [], revoke: revoke([BOARDS_READ, BOARDS_WRITE]) };
  if (to === "read") return { give: have(BOARDS_READ) ? [] : [BOARDS_READ], revoke: revoke([BOARDS_WRITE]) };
  return { give: have(BOARDS_WRITE) ? [] : [BOARDS_WRITE], revoke: [] };
}

/** The columns that are one grant each, and its power. */
export const SINGLE: Partial<Record<Column, string>> = {
  deploys: DEPLOYS_PLAN,
  start: SESSIONS_START,
  reply: SESSIONS_REPLY,
  plane: TRACKERS_WRITE,
};

/** A machine's live agent grants over a scope: of `powers`, or (without)
 * those in conversations. */
export function liveOver(grants: Grant[], machine: string, s: AgentScope, powers?: string[]): Grant[] {
  return grants.filter(
    (g) =>
      g.live &&
      g.subject.id === machine &&
      g.power !== "cloud.drive" &&
      (powers
        ? powers.includes(g.power)
        : !g.power.startsWith("runs.") && !g.power.startsWith("boards.") && !g.power.startsWith("sessions.") && !g.power.startsWith("trackers.") && g.power !== DEPLOYS_PLAN) &&
      g.scope.kind === s.kind &&
      (s.kind === "account" || g.scope.id === s.id),
  );
}

/** The machines on the account, by name: every one has had a grant since
 * it joined (cloud.drive). */
export function machinesOf(grants: Grant[]): { id: string; name: string }[] {
  const by = new Map<string, string>();
  for (const g of grants) if (g.subject.kind === "daemon") by.set(g.subject.id, g.subject.name || g.subject.id);
  return [...by].map(([id, name]) => ({ id, name })).sort((a, b) => a.name.localeCompare(b.name) || a.id.localeCompare(b.id));
}

/** A machine's live `deploys.plan` grant over a project (M218), if any. */
export function deployGrant(grants: Grant[], machine: string, project: string): Grant | undefined {
  return singleGrant(grants, machine, project, DEPLOYS_PLAN);
}

/** A machine's live grant of one `power` over a project, if any. */
export function singleGrant(grants: Grant[], machine: string, project: string, power: string): Grant | undefined {
  return grants.find((g) => g.live && g.subject.id === machine && g.power === power && g.scope.kind === "project" && g.scope.id === project);
}

/** A column of a project's matrix on /settings (./agents): one switch per
 * machine. Threads & issues is the conversation one (`powersFor`), runs and
 * commands are M216's, deploys M218's, start and reply (sessions) aru-121's, boards (#293) three levels.
 * Your DMs have only the first. */
export type Column = "threads" | "runs" | "commands" | "deploys" | "start" | "reply" | "boards" | "plane";

/** A project's columns, in the matrix's order. */
export const COLUMNS: Column[] = ["threads", "runs", "commands", "deploys", "start", "reply", "boards", "plane"];

/** One switch: on while any of its grants is live, and `only` the powers it
 * holds when that's some but not all of what it gives (a partial grant,
 * shown as "only threads.read"). `limited`: what one of its live grants
 * does on a viewer's (Arugula #683), said under it. `revoked`: a project
 * owner (not the giver) revoked one of its grants, and no live one has
 * replaced it: who and when, said under it. */
export interface Cell {
  on: boolean;
  only: string[] | null;
  limited: { label: string; why: string } | null;
  revoked: { name: string; at: number } | null;
}

/** The limit of the first of these live grants that has one, or null. */
export const limitOf = (live: Grant[]): Cell["limited"] => live.find((g) => g.limited)?.limited ?? null;

/** The limit on a machine's boards grant over a project, if any. */
export function boardsLimit(grants: Grant[], machine: string, project: string): Cell["limited"] {
  return limitOf([BOARDS_WRITE, BOARDS_READ].flatMap((p) => singleGrant(grants, machine, project, p) ?? []));
}

/** The newest of a machine's grants of `powers` over a scope that someone
 * other than its giver revoked (a project's owner, the platform, an
 * account since deleted), when no live grant of that power has replaced
 * it: who (by name, else "an owner") and when. */
export function revokedByOther(grants: Grant[], machine: string, s: AgentScope, powers: string[]): Cell["revoked"] {
  const over = grants.filter(
    (g) => g.subject.id === machine && powers.includes(g.power) && g.scope.kind === s.kind && (s.kind === "account" || g.scope.id === s.id),
  );
  const replaced = new Set(over.filter((g) => g.live).map((g) => g.power));
  const others = over.filter((g) => !g.live && g.revoked_by && g.revoked_by.account !== g.granted_by && !replaced.has(g.power));
  others.sort((a, b) => (b.revoked_at ?? 0) - (a.revoked_at ?? 0));
  const g = others[0];
  return g ? { name: g.revoked_by!.name ?? g.revoked_by!.account ?? "an owner", at: g.revoked_at ?? 0 } : null;
}

/** The boards column's `revoked`. */
export const boardsRevoked = (grants: Grant[], machine: string, s: AgentScope) => revokedByOther(grants, machine, s, [BOARDS_READ, BOARDS_WRITE]);

/** What one machine's switch in one column over a scope reads of the
 * grants. Deploys, start and reply are one grant each, so never partial. */
export function cellOf(grants: Grant[], machine: string, s: AgentScope, column: Column, role?: Role): Cell {
  if (column === "boards") {
    return {
      on: boardsLevel(grants, machine, s.id ?? "") !== "off",
      only: null,
      limited: boardsLimit(grants, machine, s.id ?? ""),
      revoked: boardsRevoked(grants, machine, s),
    };
  }
  const single = SINGLE[column];
  if (single) {
    const g = singleGrant(grants, machine, s.id ?? "", single);
    return { on: !!g, only: null, limited: g ? limitOf([g]) : null, revoked: revokedByOther(grants, machine, s, [single]) };
  }
  const powers = column === "runs" ? RUN_POWERS : column === "commands" ? COMMAND_POWERS : undefined;
  const live = liveOver(grants, machine, s, powers);
  // What the switch gives this reader: a viewer's machine isn't given the
  // writes, so holding the rest isn't partial.
  const gives = forRole(powers ?? powersFor(s.kind), role);
  const all = gives.every((p) => live.some((g) => g.power === p));
  return {
    on: live.length > 0,
    only: live.length > 0 && !all ? live.map((g) => g.power) : null,
    limited: limitOf(live),
    revoked: revokedByOther(grants, machine, s, powers ?? powersFor(s.kind)),
  };
}

/** A scope's matrix: a row per machine, a cell per column. */
export function matrixOf(grants: Grant[], machines: { id: string; name: string }[], s: AgentScope, columns: Column[], role?: Role) {
  return machines.map((machine) => ({ machine, cells: columns.map((c) => cellOf(grants, machine.id, s, c, role)) }));
}

/** Whether a column's switch has nothing to give a viewer's machine
 * (Arugula #683): the whole of what it gives is refused. */
export function columnRefused(column: Column, role?: Role): boolean {
  if (role !== "viewer") return false;
  const single = SINGLE[column];
  const gives = single ? [single] : column === "runs" ? RUN_POWERS : column === "commands" ? COMMAND_POWERS : column === "boards" ? [BOARDS_READ] : powersFor("project");
  return forRole(gives, role).length === 0;
}

/** How many of a matrix's machines have any switch on: its card's count. */
export const reaching = (rows: { cells: Cell[] }[]) => rows.filter((r) => r.cells.some((c) => c.on)).length;

/** A switch's key, for its busy state and its `data-agent-cell`:
 * machine/project/column, or machine/dms/threads for your DMs. */
export const cellKey = (machine: string, s: AgentScope, column: Column) => `${machine}/${s.id ?? "dms"}/${column}`;
