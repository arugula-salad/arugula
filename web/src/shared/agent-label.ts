// Who an agent is, beside its owner's name (M197, M230): "<owner>'s agent
// on <machine>", or an agent grant token's (`agt_…`), whose name isn't a
// machine's: "<owner>'s agent token “<its name>”". Said so wherever an
// agent's work is: a deploy's planner and log, an issue's author and
// timeline (UI audit #167, #192). An agent session (`ses_…`, aru-115) acts
// as itself, with no owner: "Agent session <its first 8 hex>", as chat
// names what one said.

type Agent = { machine: string; machine_name?: string };

/** Whether the agent is an agent grant token's, not a machine's. */
export const isToken = (agent: Agent) => agent.machine.startsWith("agt_");

/** Whether the agent is an agent session, acting as itself. */
export const isSession = (agent: Agent) => agent.machine.startsWith("ses_");

/** A token's name, without the " (MCP)" the platform names it with as a
 * machine (grants.rs's token_label). */
const tokenName = (agent: Agent) => (agent.machine_name || agent.machine).replace(/ \(MCP\)$/, "");

/** A session's name, as the platform gives it, else from its id. */
const sessionName = (agent: Agent) => agent.machine_name || `Agent session ${agent.machine.slice(4, 12)}`;

/** "<owner>'s agent on <machine>", or a token's "<owner>'s agent token
 * “<name>”", or a session's own name. `gone`: a machine with no name now. */
export function agentLabel(owner: string, agent: Agent, gone = agent.machine): string {
  if (isSession(agent)) return sessionName(agent);
  if (isToken(agent)) return `${owner}'s agent token “${tokenName(agent)}”`;
  return `${owner}'s agent on ${agent.machine_name || gone}`;
}

/** Short, for a list's row (UI audit #163): `agent · <machine>`, or
 * `agent token · <name>`, or a session's own name. */
export function agentShort(agent: Agent, gone = agent.machine): string {
  if (isSession(agent)) return sessionName(agent);
  return isToken(agent) ? `agent token · ${tokenName(agent)}` : `agent · ${agent.machine_name || gone}`;
}

/** Where an agent is, after "an agent": "on <machine>", or "token “<name>”",
 * or "session <8 hex>" (a badge's title). */
export function agentWhere(agent: Agent, gone = agent.machine): string {
  if (isSession(agent)) return `session ${agent.machine.slice(4, 12)}`;
  return isToken(agent) ? `token “${tokenName(agent)}”` : `on ${agent.machine_name || gone}`;
}
