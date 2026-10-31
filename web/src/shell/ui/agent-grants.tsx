// What this machine's agents may do on Arugula control (Discovery 10):
// *Agents' grants…* in the machine's settings (the machine menu's *This
// machine*). It reads the account's grants as `arugula grants ls` does
// (`GET /api/grants`, the platform's, on control's page) and shows this
// machine's live ones: threads, issues, runs, deploys, boards, sessions
// over a project or your DMs, and `cloud.drive`. They're changed on the
// platform's Settings, which it links to; here they're read only. On a
// machine's own page, which control's routes aren't on, it says where to
// look instead.

import { useEffect, useState } from "preact/hooks";
import { platformFetch } from "../../shared/maintenance";
import { Modal } from "../../shared/ui/modal";
import { directory } from "../hosts";
import { openPlatform, platformOrigin } from "../platform-links";

/** A grant, as `GET /api/grants` lists it. */
interface Grant {
  id: string;
  subject: { kind: string; id: string; name: string | null };
  power: string;
  scope: { kind: "project" | "account"; id: string; name?: string | null };
  live: boolean;
}

/** What each power lets an agent do, as a phrase. */
const POWERS: Record<string, string> = {
  "cloud.drive": "Drive your cloud workspaces",
  "threads.read": "Read threads",
  "threads.post": "Post in threads",
  "issues.write": "Open and change issues",
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

/** #366: where a machine holds all of `boards.write`, `issues.write` and
 * `trackers.write` is one line, "Work on boards"; the three leave their own
 * lines there. */
export function withWork(byPower: Map<string, string[]>): Map<string, string[]> {
  const three = ["boards.write", "issues.write", "trackers.write"];
  const all = (byPower.get("boards.write") ?? []).filter((w) => three.every((p) => byPower.get(p)?.includes(w)));
  if (!all.length) return byPower;
  const out = new Map(byPower);
  for (const p of three) {
    const rest = (out.get(p) ?? []).filter((w) => !all.includes(w));
    if (rest.length) out.set(p, rest);
    else out.delete(p);
  }
  out.set("work.boards", all);
  return out;
}

/** Where one reaches, in words. */
function over(g: Grant): string {
  if (g.power === "cloud.drive") return "your account";
  if (g.scope.kind === "account") return "your DMs";
  return g.scope.name ?? g.scope.id;
}

let shown = false;
const listeners = new Set<() => void>();
const changed = () => listeners.forEach((fn) => fn());

export function openAgentGrants() {
  shown = true;
  changed();
}

export function AgentGrantsLayer() {
  const [, setTick] = useState(0);
  useEffect(() => {
    const fn = () => setTick((t) => t + 1);
    listeners.add(fn);
    return () => void listeners.delete(fn);
  }, []);
  if (!shown) return null;
  return (
    <AgentGrants
      close={() => {
        shown = false;
        changed();
      }}
    />
  );
}

/** This machine's live grants, from the account's: by its id in control's
 * directory, else by its name. */
export function grantsOf(all: Grant[], machine: { id?: string; name: string }): Grant[] {
  return all.filter((g) => g.live && g.subject.kind === "daemon" && (machine.id ? g.subject.id === machine.id : g.subject.name === machine.name));
}

function AgentGrants({ close }: { close: () => void }) {
  const name = directory.current;
  const id = directory.find(name)?.id;
  const [grants, setGrants] = useState<Grant[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    let live = true;
    platformFetch("/api/grants", { credentials: "same-origin" })
      .then(async (r) => {
        const v = (await r.json().catch(() => null)) as { grants?: Grant[]; error?: string } | null;
        if (!r.ok || !v?.grants) throw new Error(v?.error ?? `${r.status}`);
        if (live) setGrants(grantsOf(v.grants, { id, name }));
      })
      .catch(() => live && setError("Control's page has them: open this machine through Control to see them here."));
    return () => void (live = false);
  }, [id, name]);
  const settings = platformOrigin();
  // By power, each with where it reaches.
  const plain = new Map<string, string[]>();
  for (const g of grants ?? []) plain.set(g.power, [...(plain.get(g.power) ?? []), over(g)]);
  const byPower = withWork(plain);
  const order = Object.keys(POWERS);
  const powers = [...byPower.keys()].sort((a, b) => (order.indexOf(a) + 1 || 99) - (order.indexOf(b) + 1 || 99) || a.localeCompare(b));
  return (
    <Modal close={close} label="Agents' grants" class="rules agent-grants">
      <h2>Agents' grants</h2>
      <p class="hint">
        What the agents on {name} may do on Arugula control, as you: each one's work there is labelled as your agent's on this machine.
      </p>
      {error && <p class="hint" data-agent-grants-error>{error}</p>}
      {!error && grants === null && <p class="hint">Loading…</p>}
      {grants && !powers.length && <p data-agent-grants-none>None: its agents can't do anything on control.</p>}
      {powers.length > 0 && (
        <ul class="agent-grants-list" data-agent-grants>
          {powers.map((p) => (
            <li key={p} data-agent-grant={p}>
              <b>{POWERS[p] ?? p}</b> <span class="hint">over {byPower.get(p)!.join(", ")}</span>
            </li>
          ))}
        </ul>
      )}
      <p class="hint">
        {settings ? "Give or take them back in the platform's Settings, under Agents, or with " : "Give or take them back with "}
        <code>arugula grants</code>.
      </p>
      <div class="prompt-buttons">
        {settings && (
          <button
            class="secondary"
            type="button"
            data-agent-grants-settings
            onClick={() => {
              close();
              openPlatform("/settings#agents");
            }}
          >
            Change them in Settings ↗
          </button>
        )}
        <button class="primary" type="button" onClick={close}>
          Done
        </button>
      </div>
    </Modal>
  );
}
