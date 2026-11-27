// Claude Code's hooks are only partly installed (#906: an older install,
// say): said once, as a line under the top bar, once per browser for each
// set of what's missing, with Set it up…, which opens Getting started (Add
// a machine) with Claude Code's one click under it (Lift 7). Only the
// owner's own daemon is asked (`/api/setup?part=agents`), once per page.
// Agent panes' adapters aren't said here: Continue here runs a conversation
// in a terminal, which needs none
// (https://app.arugula.io/issues/iss_fa949b979b607ef8f0414cbe674ab33b), and
// the Start-an-agent dialog says each adapter's state.
//
// And the hooks alone (`turnOnHooks`, `POST /api/setup/hooks`): the hint
// `claude-hooks` (../hints/hooks) on a terminal where Claude Code waits at
// a permission prompt. The bell's empty drawer offers Getting started's
// one click (`setUpClaude`).

import { opPath } from "../ops";
import { useEffect, useState } from "preact/hooks";
import type { Client } from "../client";
import { openGettingStarted, withCode, type HooksPartial } from "./welcome";

const SAID_KEY = "arugula.agents-nudge";

/** `GET /api/setup?part=agents`. */
interface AgentsSetup {
  claude: {
    installed: boolean;
    tools: boolean;
    /** #505: registered as `illogical` (works until set up again). */ old?: boolean;
    /** Arugula's hooks are in Claude Code's settings (absent: an older daemon). */
    hooks?: boolean;
    /** #395: a policy stops Claude Code running them. */
    hooks_blocked?: string;
    /** #906: only some of them are there. */
    hooks_partial?: HooksPartial;
  };
}

function said(): string[] {
  try {
    return JSON.parse(localStorage.getItem(SAID_KEY) ?? "[]") as string[];
  } catch {
    return [];
  }
}

function remember(keys: string[]) {
  try {
    localStorage.setItem(SAID_KEY, JSON.stringify([...new Set([...said(), ...keys])]));
  } catch {
    // Nothing to remember it in: said again next time.
  }
}

/** #906: hooks only partly installed, said once per browser for each
 * set of what's missing (a release that adds a hook says it again). Not
 * when a policy blocks them: installing the rest wouldn't help. */
function partialKey(s: AgentsSetup | null): string | null {
  const p = s?.claude.hooks_partial;
  if (!p || s?.claude.hooks_blocked) return null;
  return `hooks:${[...p.missing, ...p.outdated].join(",")}${p.rules ? "+rules" : ""}`;
}

export function AgentsNudge({ client }: { client: Client }) {
  const mine = !client.e2e && !client.state?.roles && !navigator.webdriver;
  const [partial, setPartial] = useState<string | null>(null);
  useEffect(() => {
    if (!mine) return;
    void agentsSetup().then((s) => {
        const hooks = partialKey(s);
        if (!hooks || said().includes(hooks)) return;
        // Once: shown on this page until acted on, not on the next.
        remember([hooks]);
        setPartial(s?.claude.hooks_partial?.says ?? null);
      });
  }, [mine]);
  if (!partial) return null;
  return (
    <div class="agents-nudge" role="status" data-agents-nudge="hooks">
      <p>
        <span data-agents-nudge-hooks>{withCode(partial)}</span>
      </p>
      <div class="agents-nudge-actions">
        <button
          class="primary"
          data-agents-nudge-setup
          onClick={() => {
            setPartial(null);
            openGettingStarted("agents", client);
          }}
        >
          Set it up…
        </button>
        <button onClick={() => setPartial(null)} data-agents-nudge-hide>
          Not now
        </button>
      </div>
    </div>
  );
}

/** What the daemon says of its agents' setup (`/api/setup?part=agents`):
 * asked once per page, by the nudge and the hooks offer alike. */
let setupHere: Promise<AgentsSetup | null> | null = null;

function agentsSetup(): Promise<AgentsSetup | null> {
  setupHere ??= fetch(`${opPath("setup.get")}?part=agents`)
    .then((r) => (r.ok ? (r.json() as Promise<AgentsSetup>) : null))
    .catch(() => null);
  return setupHere;
}

/** Whether Claude Code's settings have Arugula's hooks, as the daemon last
 * said (or since a click here added them). */
let hooksHere: Promise<boolean> | null = null;

export function hooksInstalled(): Promise<boolean> {
  hooksHere ??= agentsSetup().then((s) => s?.claude.hooks !== false);
  return hooksHere;
}

/** Getting started's one click for Claude Code (`POST
 * /api/setup/agents/claude`, which installs the hooks too): whether it
 * worked, and what to say. */
export async function setUpClaude(): Promise<{ ok: boolean; said: string }> {
  const o = await fetch(opPath("setup.agent", { kind: "claude" }), { method: "POST", headers: { "content-type": "application/json" }, body: "{}" })
    .then((r) => r.json() as Promise<{ ok: boolean; error?: string; done?: string[] }>)
    .catch((e: unknown) => ({ ok: false, error: String(e), done: [] }));
  if (o.ok) hooksHere = Promise.resolve(true);
  const hooks = o.done?.find((d) => d.includes("hooks")) ?? o.done?.[0];
  return { ok: o.ok, said: o.ok ? (hooks ?? "Set up.") : (o.error ?? "Setting it up didn't work.") };
}

/** The hooks offer's one click (`POST /api/setup/hooks`: `arugula hooks
 * install`, nothing else), said in a toast that names the file it changed. */
export async function turnOnHooks(client: Client) {
  const o = await fetch(opPath("setup.hooks"), { method: "POST", headers: { "content-type": "application/json" }, body: "{}" })
    .then((r) => r.json() as Promise<{ ok: boolean; error?: string; fix?: string; done?: string[] }>)
    .catch((e: unknown) => ({ ok: false, error: String(e), fix: undefined, done: [] }));
  if (o.ok) hooksHere = Promise.resolve(true);
  client.toast(o.ok ? (o.done ?? []).join(" ") || "Claude Code has Arugula's hooks." : `Couldn't turn them on: ${o.error ?? "it didn't say why"}${o.fix ? `. Run ${o.fix}` : ""}`);
}
