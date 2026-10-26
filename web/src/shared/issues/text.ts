// What an issue's page says (M201): a timeline event as a line, who did
// it (a person, or "<owner>'s agent on <machine>", M197), how long ago,
// and what a list asks the platform for. Plain functions, for the page
// and its tests (scripts/issues.test.mjs).

import type { ChatAgent, IssueEvent, NativeIssue } from "../proto";

/** An account's name now, or null for someone the page doesn't know. */
export type NameOf = (account: string) => string | null | undefined;

/** Who did something: a person by name, or their agent (M197). */
export function whoLabel(account: string, agent: ChatAgent | null, nameOf: NameOf): string {
  const name = account ? nameOf(account) || "someone who left" : "a deleted account";
  return agent ? `${name}'s agent on ${agent.machine_name || "a machine that left"}` : name;
}

/** Who, short, for a list's row (UI audit #163): a person by name, an
 * agent as `agent · <machine>`. */
export function whoShort(account: string, agent: ChatAgent | null, nameOf: NameOf): string {
  if (agent) return `agent · ${agent.machine_name || "a machine that left"}`;
  return account ? nameOf(account) || "someone who left" : "a deleted account";
}

/** A list row's line after its number: `opened 3 days ago by kim`, or a
 * closed one's `closed as completed by kim 2 days ago` (UI audit #163). */
export function rowLine(i: Pick<NativeIssue, "state" | "close_reason" | "author" | "agent" | "created_at" | "closed_at" | "closed_by" | "closed_by_agent">, nameOf: NameOf, now = Date.now()): string {
  if (i.state !== "closed" || !i.closed_at) return `opened ${ago(i.created_at, now)} by ${whoShort(i.author, i.agent, nameOf)}`;
  const by = i.closed_by !== undefined ? ` by ${whoShort(i.closed_by, i.closed_by_agent ?? null, nameOf)}` : "";
  return `closed as ${reasonLabel(i.close_reason)}${by} ${ago(i.closed_at, now)}`;
}

const REASONS: Record<string, string> = {
  completed: "completed",
  not_planned: "not planned",
  duplicate: "a duplicate",
};

/** Why a closed issue is closed, as words. */
export function reasonLabel(reason: string | null | undefined): string {
  return REASONS[reason ?? "completed"] ?? reason ?? "completed";
}

/** A closed issue's state, as its tag says it. */
export function stateLabel(i: Pick<NativeIssue, "state" | "close_reason">): string {
  if (i.state !== "closed") return "Open";
  return i.close_reason === "not_planned" ? "Closed: not planned" : i.close_reason === "duplicate" ? "Closed: duplicate" : "Closed";
}

const str = (v: unknown): string => (typeof v === "string" ? v : "");

/** What an event did, after who did it: "closed this as not planned".
 * Null for `opened`, which the issue's head already says. */
export function eventLine(e: IssueEvent, nameOf: NameOf): string | null {
  const d = e.detail ?? {};
  switch (e.kind) {
    case "opened":
      return null;
    case "edited": {
      const said: string[] = [];
      if (typeof d.title === "string") said.push(`changed the title from “${d.title}”`);
      if (d.body === true) said.push("edited the description");
      return said.join(" and ") || "edited this";
    }
    case "closed":
      return `closed this as ${reasonLabel(str(d.reason) || null)}`;
    case "reopened":
      return "reopened this";
    case "labelled":
      return `added the label ${str(d.label)}`;
    case "unlabelled":
      return `removed the label ${str(d.label)}`;
    case "assigned":
    case "unassigned": {
      const who = str(d.assignee);
      const self = who !== "" && who === e.actor && !e.agent;
      if (e.kind === "assigned") return self ? "self-assigned this" : `assigned ${nameOf(who) || "someone who left"}`;
      return self ? "unassigned themselves" : `unassigned ${nameOf(who) || "someone who left"}`;
    }
    case "exported":
      return `sent this to GitHub as ${str(d.repo)}#${typeof d.number === "number" ? d.number : "?"}`;
    default:
      return e.kind;
  }
}

/** "just now", "5 minutes ago", "3 days ago", else the date. */
export function ago(at: number, now = Date.now()): string {
  const s = Math.max(0, Math.round((now - at) / 1000));
  const n = (v: number, unit: string) => `${v} ${unit}${v === 1 ? "" : "s"} ago`;
  if (s < 45) return "just now";
  if (s < 3600) return n(Math.max(1, Math.round(s / 60)), "minute");
  if (s < 86_400) return n(Math.round(s / 3600), "hour");
  if (s < 30 * 86_400) return n(Math.round(s / 86_400), "day");
  const d = new Date(at);
  return `on ${d.toLocaleDateString([], { month: "short", day: "numeric", ...(d.getFullYear() === new Date(now).getFullYear() ? {} : { year: "numeric" }) })}`;
}

/** Labels typed in a box: comma-separated, trimmed, once each. */
export function labelsOf(text: string): string[] {
  return [...new Set(text.split(",").map((l) => l.trim()).filter(Boolean))];
}

/** What a list asks for: `open` or `closed`, a label, an assignee
 * (an account), search words. */
export interface IssueFilter {
  state?: "closed";
  label?: string;
  assignee?: string;
  q?: string;
}

/** A list's query string. */
export function queryOf(f: IssueFilter, before?: number, limit?: number): string {
  const q = new URLSearchParams({ state: f.state ?? "open" });
  if (f.label) q.set("labels", f.label);
  if (f.assignee) q.set("assignee", f.assignee);
  if (f.q?.trim()) q.set("q", f.q.trim());
  if (before !== undefined) q.set("before", String(before));
  if (limit !== undefined) q.set("limit", String(limit));
  return q.toString();
}

/** GitHub's page for a new issue on `owner/name`, each part escaped. */
export function githubNewIssue(repo: string): string {
  const [owner = "", name = ""] = repo.split("/");
  return `https://github.com/${encodeURIComponent(owner)}/${encodeURIComponent(name)}/issues/new`;
}
