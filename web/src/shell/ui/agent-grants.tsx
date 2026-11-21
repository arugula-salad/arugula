// What this machine's agents may do on Arugula cloud (Discovery 10): the
// words for the grants `GET /api/grants` lists (`arugula grants ls`), as
// Access shows this machine's live ones. They're given and taken back in
// Settings › Machines, under Agents (Arugula #1107): one editable view
// where there were two (that and a read-only dialog here).

/** A grant, as `GET /api/grants` lists it. */
export interface Grant {
  id: string;
  subject: { kind: string; id: string; name: string | null };
  power: string;
  scope: { kind: "project" | "team" | "account"; id: string; name?: string | null };
  live: boolean;
}

/** What each power lets an agent do, as a phrase. */
export const POWERS: Record<string, string> = {
  "cloud.drive": "Drive your cloud workspaces",
  "threads.read": "Read threads",
  "threads.post": "Post in threads",
  "issues.write": "Open and change issues",
  "docs.write": "Write Docs",
  "deploys.plan": "Plan deploys and ask for their approval",
  "runs.read": "Read runs",
  "runs.start": "Start run specs",
  "runs.command": "Start commands and instructions as runs",
  "boards.read": "Read boards",
  "boards.write": "Make and change boards",
  "work.boards": "Work on boards: add, attach and ask on them, file issues and write to Plane",
  "trackers.write": "Write to your connected Plane projects",
  "sessions.start": "Start agent sessions as you",
  "sessions.reply": "Answer and prompt agent sessions",
};

/** Where one reaches, in words. */
export function over(g: Grant): string {
  if (g.power === "cloud.drive") return "your account";
  if (g.scope.kind === "account") return "your DMs";
  return g.scope.name ?? g.scope.id;
}

/** This machine's live grants, from the account's: by its id in control's
 * directory, else by its name. */
export function grantsOf(all: Grant[], machine: { id?: string; name: string }): Grant[] {
  return all.filter((g) => g.live && g.subject.kind === "daemon" && (machine.id ? g.subject.id === machine.id : g.subject.name === machine.name));
}
