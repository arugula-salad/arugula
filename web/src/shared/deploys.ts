// What the platform page and control's deploy prompt both say of a deploy
// (UI audit #183, #167): its state in words, and its source or digest
// short. Control's page can't reach platform/, so they're here.

import { agentLabel } from "./agent-label.ts";

const DEPLOY: Record<string, string> = {
  planned: "Planned",
  pending: "Waiting for approval",
  approved: "Approved",
  denied: "Denied",
  canceled: "Canceled",
  expired: "Expired",
  running: "Being applied",
  succeeded: "Applied",
  failed: "Failed",
  stale: "Stale",
};

/** A deploy's state, as people read it. */
export const deployStateText = (state: string) => DEPLOY[state] ?? state;

/** The state each event in a deploy's log leaves it in
 * (`EventKind::to` in crates/platform/src/db/deploys.rs). */
const EVENT_TO: Record<string, string> = {
  requested: "pending",
  started: "running",
  staled: "stale",
};

/** The state a deploy's log event moved it to: its log reads in the
 * header's words ("Applied", not "succeeded"). */
export const deployEventState = (kind: string) => EVENT_TO[kind] ?? kind;

/** A commit's or an artifact's reference, or a digest, short: 12 hex, after
 * an artifact's `sha256:` (UI audit #167: "sha256:c09af" said nothing). */
export const shortRef = (ref: string) => ref.replace(/^sha256:/, "").slice(0, 12);

/** Who planned a deploy, from their name: a person; "<owner>'s agent on
 * <machine>"; or an agent token's (M230, `agt_…`), whose name isn't a
 * machine's: "<owner>'s agent token “<its name>”" (UI audit #167). */
export function plannerText(who: string, agent: { machine: string; machine_name?: string } | null): string {
  return agent ? agentLabel(who, agent) : who;
}

/** What a deploy is called (#333): the commit's subject and, for the
 * release driver, the version its run is expected to tag, which the
 * platform read when it planned the deploy. Not in the signed operation:
 * a page shows it as the platform's reading. The same words as
 * `titles::title` in crates/platform/src/drivers/titles.rs. */
export function deployTitle(d: {
  environment: { name: string; driver: string };
  source: { ref: string };
  subject?: string | null;
  expected_version?: string | null;
}): string {
  const short = d.source.ref.slice(0, 8);
  const subject = d.subject ? ` · ${d.subject}` : "";
  if (d.expected_version && d.environment.driver === "arugula-release") return `Release ${d.expected_version} (expected) · ${short}${subject}`;
  return d.subject ? `Deploy ${short} to ${d.environment.name}${subject}` : `Deploy to ${d.environment.name}`;
}

/** How long a deploy has waited since it was asked for: "3h", "12m",
 * "2d". */
export function waitedText(since: number, now = Date.now()): string {
  const m = Math.max(0, Math.floor((now - since) / 60_000));
  if (m < 1) return "under a minute";
  if (m < 60) return `${m}m`;
  if (m < 24 * 60) return `${Math.floor(m / 60)}h`;
  return `${Math.floor(m / (24 * 60))}d`;
}
