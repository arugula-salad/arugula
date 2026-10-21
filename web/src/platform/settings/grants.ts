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

/** A machine's live agent grants over a scope. */
export function liveOver(grants: Grant[], machine: string, s: AgentScope): Grant[] {
  return grants.filter(
    (g) =>
      g.live &&
      g.subject.id === machine &&
      g.power !== "cloud.drive" &&
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
