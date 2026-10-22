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
