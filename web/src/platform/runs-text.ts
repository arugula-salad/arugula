// What the runs page (M215) says of a run, as plain functions: the exact
// command its S37 operation runs (decoded from the operation's bytes, the
// same bytes its hash names), how it was started, its state, where its
// pane is on control's page, and the address's filters. Tested in
// scripts/runs.test.mjs.

/** What a run's operation does (`arugula_e2e::run::Op`). */
export interface Op {
  daemon: string;
  kind: string;
  argv: string[];
  instruction: string | null;
  agent: string | null;
  directory: string | null;
  /** A deploy's apply (M219): which deploy, by which driver, and the
   * hash of the operation its approval signs. */
  deploy: { deploy: string; driver: string; apply: string } | null;
}

function unb64(s: string): string | null {
  try {
    const bin = atob(s);
    return new TextDecoder("utf-8", { fatal: true }).decode(Uint8Array.from(bin, (c) => c.charCodeAt(0)));
  } catch {
    return null;
  }
}

/** An `arugula op v1` operation's parts, or null when it isn't one (the
 * page then shows its bytes as they are). */
export function decodeOp(op: string): Op | null {
  if (!op.endsWith("\n")) return null;
  const lines = op.slice(0, -1).split("\n");
  if (lines[0] !== "arugula op v1") return null;
  const out: Op = { daemon: "", kind: "", argv: [], instruction: null, agent: null, directory: null, deploy: null };
  for (const line of lines.slice(1)) {
    const at = line.indexOf(" ");
    if (at < 0) return null;
    const [name, value] = [line.slice(0, at), line.slice(at + 1)];
    if (name === "daemon") out.daemon = value;
    else if (name === "kind") out.kind = value;
    else if (name === "session" || name === "pane") continue;
    else if (name === "cwd") {
      if (value === "-") continue;
      out.directory = unb64(value);
      if (out.directory === null) return null;
    } else if (name === "argv") {
      const a = unb64(value);
      if (a === null) return null;
      out.argv.push(a);
    } else if (name === "text") {
      out.instruction = unb64(value);
      if (out.instruction === null) return null;
    } else if (name === "agent") out.agent = value === "-" ? null : value;
    else if (name === "deploy") out.deploy = { deploy: value, driver: "", apply: "" };
    else if (name === "driver" && out.deploy) out.deploy.driver = value;
    else if (name === "apply" && out.deploy) out.deploy.apply = value;
    else return null;
  }
  // A deploy's apply is that and nothing else (UI audit #145).
  if (out.deploy) return out.argv.length || out.instruction !== null || out.directory !== null ? null : out;
  return out.argv.length || out.instruction !== null ? out : null;
}

/** One argument as a shell would read it back. */
function quote(a: string): string {
  return a !== "" && /^[A-Za-z0-9_@%+=:,./-]+$/.test(a) ? a : `'${a.replace(/'/g, `'\\''`)}'`;
}

/** A command, as typed in a shell. */
export function commandLine(argv: string[]): string {
  return argv.map(quote).join(" ");
}

/** A command typed in a box, as its arguments: words split on spaces,
 * with '…' and "…" keeping theirs together. Null when a quote isn't
 * closed. */
export function argvOf(line: string): string[] | null {
  const out: string[] = [];
  let cur: string | null = null;
  let q: string | null = null;
  for (const ch of line) {
    if (q) {
      if (ch === q) q = null;
      else cur += ch;
    } else if (ch === "'" || ch === '"') {
      q = ch;
      cur ??= "";
    } else if (/\s/.test(ch)) {
      if (cur !== null) out.push(cur);
      cur = null;
    } else cur = (cur ?? "") + ch;
  }
  if (q) return null;
  if (cur !== null) out.push(cur);
  return out;
}

/** The deploy a deploy's run applies, as the platform names it
 * (`RunView.deploy`). */
export interface RunDeploy {
  id: string;
  environment: string;
  source_kind: string;
  reference: string;
}

/** A deploy's source, short: a commit's first 7, a digest's first 12 hex. */
export function shortSource(reference: string): string {
  const at = reference.indexOf(":");
  return at >= 0 ? reference.slice(at + 1, at + 13) : reference.slice(0, 7);
}

/** A deploy's run, in a line: `Deploy production · 1a2b3c4`. */
export const deployTitle = (d: RunDeploy) => `Deploy ${d.environment} · ${shortSource(d.reference)}`;

/** What a run did, in a line: its spec's name, its deploy (UI audit #145),
 * else its command or the first line of its instruction. */
export function runTitle(op: string, specName?: string | null, deploy?: RunDeploy | null): string {
  if (specName) return specName;
  if (deploy) return deployTitle(deploy);
  const o = decodeOp(op);
  if (!o) return "a run";
  if (o.deploy) return `Deploy ${o.deploy.deploy}`;
  return o.instruction !== null ? (o.instruction.split("\n")[0] ?? "") : commandLine(o.argv);
}

/** A run's workspace as pages name it (UI audit #145): its name, a deploy's
 * own box as that, or its id when the platform sent no name. */
export function workspaceLabel(run: { workspace?: string | null; workspace_name?: string | null; deploy?: RunDeploy | null }): string | null {
  if (!run.workspace) return null;
  if (run.deploy) return "its deploy's box";
  return run.workspace_name || run.workspace;
}

/** S37's eight states, as the page says them. */
export const STATES: Record<string, string> = {
  pending: "Pending",
  approved: "Approved",
  running: "Running",
  succeeded: "Succeeded",
  failed: "Failed",
  denied: "Denied",
  canceled: "Canceled",
  expired: "Expired",
};

/** What starts runs, as the Trigger filter offers them (UI audit #164).
 * `deploy` is a deploy's run (M219), `manual` underneath: the platform's
 * list tells them apart. */
export const TRIGGERS: Record<string, string> = { manual: "Manual", schedule: "Scheduled", event: "Event", deploy: "Deploy" };

/** What started a run, as TRIGGERS names it: a deploy's run is `deploy`. */
export function triggerOf(run: { trigger: string; source: Record<string, unknown>; deploy?: unknown }): string {
  return run.deploy || (run.trigger === "manual" && typeof run.source?.deploy === "string") ? "deploy" : run.trigger;
}

/** A run's trigger and who, in a line (UI audit #164): `Manual · kim`,
 * `Deploy · kim`, `Scheduled`, `Event · push on o/r`. */
export function triggerLine(
  run: { trigger: string; started_by: string; started_by_agent: string | null; source: Record<string, unknown>; deploy?: unknown },
  name: (account: string) => string | undefined,
): string {
  const t = triggerOf(run);
  const word = TRIGGERS[t] ?? t;
  if (t === "manual" || t === "deploy") return `${word} · ${startedBy(run, name)}`;
  if (t === "event") {
    const s = run.source ?? {};
    const ev = typeof s.event === "string" ? s.event.replace(/_/g, " ") : "";
    const repo = typeof s.repo === "string" ? ` on ${s.repo}` : "";
    return ev ? `${word} · ${ev}${repo}` : word;
  }
  return word;
}

/** Nothing leaves it. */
export const ended = (state: string) => ["succeeded", "failed", "denied", "canceled", "expired"].includes(state);

/** How a run's dot looks: going, good, bad or neither. */
export function tone(state: string): "going" | "ok" | "bad" | "off" {
  if (state === "pending" || state === "approved" || state === "running") return "going";
  if (state === "succeeded") return "ok";
  if (state === "failed") return "bad";
  return "off";
}

/** How long it ran (or has run), in words: `42s`, `3m 5s`, `1h 2m`. */
export function took(from: number, to: number): string {
  const s = Math.max(0, Math.round((to - from) / 1000));
  if (s < 60) return `${s}s`;
  if (s < 3600) return `${Math.floor(s / 60)}m ${s % 60}s`;
  return `${Math.floor(s / 3600)}h ${Math.floor((s % 3600) / 60)}m`;
}

/** Who or what started it: a person (by hand, or an agent of theirs), a
 * schedule or a rule. `name` names an account. `start`: it begins a
 * sentence (a timeline row's subject, UI audit #164), so what isn't a
 * person's name is capitalised. */
export function startedBy(
  run: { trigger: string; started_by: string; started_by_agent: string | null; source: Record<string, unknown> },
  name: (account: string) => string | undefined,
  start = false,
): string {
  const s = run.source ?? {};
  const cap = (t: string) => (start ? `${t.charAt(0).toUpperCase()}${t.slice(1)}` : t);
  if (run.trigger === "schedule") return cap("its schedule");
  if (run.trigger === "event") {
    const ev = typeof s.event === "string" ? s.event.replace(/_/g, " ") : "an event";
    const repo = typeof s.repo === "string" ? ` on ${s.repo}` : "";
    return cap(`a rule (${ev}${repo})`);
  }
  const who = run.started_by ? (name(run.started_by) ?? cap("someone who left")) : cap("someone who left");
  return run.started_by_agent ? `${who}'s agent` : who;
}

/** An id short, its prefix kept: `hw_fa79e709` (UI audit #164). The page
 * puts the whole one in a title. */
export function shortId(id: string): string {
  const at = id.indexOf("_");
  return at >= 0 && at < 6 ? id.slice(0, at + 9) : id.slice(0, 8);
}

/** The run's pane on control's page, where its machine is: the shell's
 * `#pane=<machine>.<pane>` address. Null before it runs in one. */
export function paneLink(control: string, run: { daemon: string; pane: number | null }): string | null {
  return run.pane === null || run.pane === undefined ? null : `${control}/#pane=${run.daemon}.${run.pane}`;
}

/** What an event's detail says, in words. */
export function eventDetail(d: Record<string, unknown>): string {
  const parts: string[] = [];
  if (typeof d.reason === "string") parts.push(d.reason.replace(/^[a-z_]+: /, ""));
  if (typeof d.exit === "number") parts.push(`exit ${d.exit}`);
  if (typeof d.pane === "number") parts.push(`in pane %${d.pane}`);
  if (typeof d.approval === "string") parts.push(`approval ${d.approval}`);
  if (typeof d.error === "string") parts.push(d.error);
  return parts.join(" · ");
}

/** The runs page's filters, as its address keeps them. */
export interface RunFilter {
  state?: string;
  trigger?: string;
  spec?: string;
}

/** A filter's query for the platform's list (`GET /api/projects/{p}/runs`). */
export function runsQuery(f: RunFilter, limit = 50): string {
  const q = new URLSearchParams();
  if (f.state) q.set("state", f.state);
  if (f.trigger) q.set("trigger", f.trigger);
  if (f.spec) q.set("spec", f.spec);
  q.set("limit", String(limit));
  return q.toString();
}

/** A log's terminal escapes taken out, so it reads as text: CSI (colors,
 * cursor moves), OSC (titles, links; to BEL or ST), character sets, and the
 * other short escapes (`ESC 7`, `ESC =`…). Carriage returns before a
 * newline go too. */
export function stripAnsi(s: string): string {
  return s
    .replace(/\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)/g, "")
    .replace(/\x1b\[[0-?]*[ -/]*[@-~]/g, "")
    .replace(/\x1b[()*+][0-~]/g, "")
    .replace(/\x1b[0-~]/g, "")
    .replace(/\r+\n/g, "\n");
}

/** The note a pane writes when what it ran ended and it waits for Enter
 * (arugula_mux's pane: `[exited with code 0 · press Enter for a shell]`):
 * the terminal's, not the run's, so a kept log ends without it (UI audit
 * #146). `text` is already without its escapes. */
export function withoutExitNote(text: string): string {
  return text.replace(/\n*\[(?:exited with code -?\d+|process ended by signal \d+) · press Enter for a shell\]\s*$/, "\n");
}

/** The most of a followed log a run's view keeps (about 4 MiB): older
 * pages go first, and Earlier reads them again. */
export const LOG_KEPT = 4 * 1024 * 1024;

/** `pages` with `next` after them, dropping the oldest while what's kept
 * is past `most` characters (never `next` itself). */
export function keepLatest<T extends { text: string }>(pages: T[], next: T, most = LOG_KEPT): T[] {
  const out = [...pages, next];
  let size = out.reduce((n, p) => n + p.text.length, 0);
  while (out.length > 1 && size > most) size -= out.shift()!.text.length;
  return out;
}

/** Whether `url` may be a link: an https URL, nothing else (a run's chant
 * ledger entry, M220, is what its box reported). */
export function httpsOnly(url: string): boolean {
  try {
    return new URL(url).protocol === "https:";
  } catch {
    return false;
  }
}
