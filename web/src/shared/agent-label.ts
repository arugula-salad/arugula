// Who an agent is, beside its owner's name (M197, M230): "<owner>'s agent
// on <machine>", or an agent grant token's (`agt_…`), whose name isn't a
// machine's: "<owner>'s agent token “<its name>”". Said so wherever an
// agent's work is: a deploy's planner and log, an issue's author and
// timeline (UI audit #167, #192).

type Agent = { machine: string; machine_name?: string };

/** Whether the agent is an agent grant token's, not a machine's. */
export const isToken = (agent: Agent) => agent.machine.startsWith("agt_");

/** A token's name, without the " (MCP)" the platform names it with as a
 * machine (grants.rs's token_label). */
const tokenName = (agent: Agent) => (agent.machine_name || agent.machine).replace(/ \(MCP\)$/, "");

/** "<owner>'s agent on <machine>", or a token's "<owner>'s agent token
 * “<name>”". `gone`: a machine with no name now. */
export function agentLabel(owner: string, agent: Agent, gone = agent.machine): string {
  if (isToken(agent)) return `${owner}'s agent token “${tokenName(agent)}”`;
  return `${owner}'s agent on ${agent.machine_name || gone}`;
}

/** Short, for a list's row (UI audit #163): `agent · <machine>`, or
 * `agent token · <name>`. */
export function agentShort(agent: Agent, gone = agent.machine): string {
  return isToken(agent) ? `agent token · ${tokenName(agent)}` : `agent · ${agent.machine_name || gone}`;
}

/** Where an agent is, after "an agent": "on <machine>", or "token “<name>”"
 * (a badge's title). */
export function agentWhere(agent: Agent, gone = agent.machine): string {
  return isToken(agent) ? `token “${tokenName(agent)}”` : `on ${agent.machine_name || gone}`;
}
