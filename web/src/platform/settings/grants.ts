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
}

/** Where a switch reaches: a project, or your DMs (`account`). */
export interface AgentScope {
  kind: "project" | "account";
  id?: string;
  name: string;
}

/** What a switch gives over each scope. */
export function powersFor(kind: AgentScope["kind"]): string[] {
  return kind === "project" ? ["threads.read", "threads.post", "issues.write"] : ["threads.read", "threads.post"];
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

/** A machine's live agent grants over a scope: of `powers`, or (without)
 * those in conversations. */
export function liveOver(grants: Grant[], machine: string, s: AgentScope, powers?: string[]): Grant[] {
  return grants.filter(
    (g) =>
      g.live &&
      g.subject.id === machine &&
      g.power !== "cloud.drive" &&
      (powers ? powers.includes(g.power) : !g.power.startsWith("runs.") && g.power !== DEPLOYS_PLAN) &&
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
  return grants.find((g) => g.live && g.subject.id === machine && g.power === DEPLOYS_PLAN && g.scope.kind === "project" && g.scope.id === project);
}

/** A column of a project's matrix on /settings (./agents): one switch per
 * machine. Threads & issues is the conversation one (`powersFor`), runs and
 * commands are M216's, deploys M218's. Your DMs have only the first. */
export type Column = "threads" | "runs" | "commands" | "deploys";

/** A project's columns, in the matrix's order. */
export const COLUMNS: Column[] = ["threads", "runs", "commands", "deploys"];

/** One switch: on while any of its grants is live, and `only` the powers it
 * holds when that's some but not all of what it gives (a partial grant,
 * shown as "only threads.read"). */
export interface Cell {
  on: boolean;
  only: string[] | null;
}

/** What one machine's switch in one column over a scope reads of the
 * grants. Deploys is one grant, so it's never partial. */
export function cellOf(grants: Grant[], machine: string, s: AgentScope, column: Column): Cell {
  if (column === "deploys") return { on: !!deployGrant(grants, machine, s.id ?? ""), only: null };
  const powers = column === "runs" ? RUN_POWERS : column === "commands" ? COMMAND_POWERS : undefined;
  const live = liveOver(grants, machine, s, powers);
  const all = (powers ?? powersFor(s.kind)).every((p) => live.some((g) => g.power === p));
  return { on: live.length > 0, only: live.length > 0 && !all ? live.map((g) => g.power) : null };
}

/** A scope's matrix: a row per machine, a cell per column. */
export function matrixOf(grants: Grant[], machines: { id: string; name: string }[], s: AgentScope, columns: Column[]) {
  return machines.map((machine) => ({ machine, cells: columns.map((c) => cellOf(grants, machine.id, s, c)) }));
}

/** How many of a matrix's machines have any switch on: its card's count. */
export const reaching = (rows: { cells: Cell[] }[]) => rows.filter((r) => r.cells.some((c) => c.on)).length;

/** A switch's key, for its busy state and its `data-agent-cell`:
 * machine/project/column, or machine/dms/threads for your DMs. */
export const cellKey = (machine: string, s: AgentScope, column: Column) => `${machine}/${s.id ?? "dms"}/${column}`;
