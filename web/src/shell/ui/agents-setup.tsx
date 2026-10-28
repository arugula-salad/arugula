// #335: Claude Code (or Codex) is on this machine, and its ACP adapter
// isn't installed or is older than the daemon's pin, so agent panes won't
// start (or run an old one). Said once, as a line under the top bar, with
// Set it up…, which opens Getting started's Agents step (its one click,
// "Use Claude Code with Arugula"). Only the owner's own daemon is asked
// (`/api/setup?part=agents`), once per page; each adapter's pin is said
// once per browser, so a release that moves the pin says it again.
//
// And a terminal's: Claude Code waits in it for you, and none of Arugula's
// hooks has spoken from it (`PaneInfo.unhooked`, the daemon's word), so its
// questions stay on its screen. A one-line chip on the pane says they could
// be cards, with Set up: the same one click (`POST
// /api/setup/agents/claude`, which installs the hooks too). A one-off for
// now, kept apart so it can move into the hints when they come. The bell's
// empty drawer offers the same Set up (`setUpClaude`). Or the plugin
// shows Claude Code's plugin's two `/plugin` lines under it, the other way
// to the hooks (Discovery 10).

import { opPath } from "../ops";
import { useEffect, useState } from "preact/hooks";
import type { Client } from "../client";
import type { PaneId } from "../../shared/proto";
import { adapterReady, type Adapter } from "./adapter";
import { openGettingStarted } from "./welcome";
import { ClaudePlugin } from "./claude-plugin";

const SAID_KEY = "arugula.agents-nudge";

/** `GET /api/setup?part=agents`. */
interface AgentsSetup {
  claude: {
    installed: boolean;
    tools: boolean;
    /** #505: registered as `illogical` (works until set up again). */ old?: boolean;
    /** Arugula's hooks are in Claude Code's settings (absent: an older daemon). */
    hooks?: boolean;
  };
  adapters?: Adapter[];
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

const keyOf = (a: Adapter) => `${a.kind}@${a.pinned}`;

/** The adapters worth a word: their agent is here, and they aren't. */
export function nudged(s: AgentsSetup | null): Adapter[] {
  return (s?.adapters ?? []).filter((a) => a.found && !adapterReady(a));
}

function line(a: Adapter, tools: boolean, hooks: boolean): string {
  const mcp =
    a.kind !== "claude"
      ? ""
      : !tools
        ? `, and Arugula's MCP server lets it start its helpers as panes${hooks ? "" : " (its hooks put its questions on cards)"}`
        : !hooks
          ? ", and its hooks put its questions on cards"
          : "";
  if (a.state === "installed") return `${a.label}'s adapter here is ${a.version}, older than this Arugula's (${a.pinned})${mcp}.`;
  if (a.state === "no_node") return `${a.label} is on this machine, but agent panes can't run it: ${a.why ?? `its adapter needs Node ${a.node_major}+`}.`;
  return `${a.label} is on this machine, but agent panes need its adapter${mcp}.`;
}

export function AgentsNudge({ client }: { client: Client }) {
  const mine = !client.e2e && !client.state?.roles && !navigator.webdriver;
  const [setup, setSetup] = useState<AgentsSetup | null>(null);
  const [shown, setShown] = useState<Adapter[]>([]);
  useEffect(() => {
    if (!mine) return;
    fetch(`${opPath("setup.get")}?part=agents`)
      .then((r) => (r.ok ? (r.json() as Promise<AgentsSetup>) : null))
      .then((s) => {
        const before = said();
        const fresh = nudged(s).filter((a) => !before.includes(keyOf(a)));
        if (!fresh.length) return;
        // Once: shown on this page until acted on, not on the next.
        remember(fresh.map(keyOf));
        setSetup(s);
        setShown(fresh);
      })
      .catch(() => {});
  }, [mine]);
  if (!shown.length || !setup) return null;
  const tools = setup.claude.tools;
  const hooks = setup.claude.hooks !== false;
  return (
    <div class="agents-nudge" role="status" data-agents-nudge={shown.map((a) => a.kind).join(" ")}>
      <p>
        {shown.map((a) => (
          <span key={a.kind}>{line(a, tools, hooks)} </span>
        ))}
      </p>
      <div class="agents-nudge-actions">
        <button
          class="primary"
          data-agents-nudge-setup
          onClick={() => {
            setShown([]);
            openGettingStarted("agents", client);
          }}
        >
          Set it up…
        </button>
        <button onClick={() => setShown([])} data-agents-nudge-hide>
          Not now
        </button>
      </div>
    </div>
  );
}

/** Whether Claude Code's settings have Arugula's hooks, as the daemon last
 * said (`/api/setup?part=agents`): asked once per page, when a chip would
 * show. */
let hooksHere: Promise<boolean> | null = null;

export function hooksInstalled(): Promise<boolean> {
  hooksHere ??= fetch(`${opPath("setup.get")}?part=agents`)
    .then((r) => (r.ok ? (r.json() as Promise<AgentsSetup>) : null))
    .then((s) => s?.claude.hooks !== false)
    .catch(() => true);
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

/** Panes whose chip was put away, for this page. */
const putAway = new Set<PaneId>();

/** The chip on a terminal where Claude Code waits for you and no hook has
 * spoken from it: only the owner's own daemon can set it up, and only
 * while its settings lack the hooks (a Claude started before them needs a
 * restart, which Set up can't do). */
export function HooksChip({ client, id }: { client: Client; id: PaneId }) {
  const info = client.info(id);
  const mine = !client.e2e && !client.state?.roles;
  const wants = mine && !!info?.unhooked && info.attention === "needs_input" && !putAway.has(id);
  const [missing, setMissing] = useState(false);
  const [busy, setBusy] = useState(false);
  const [said, setSaid] = useState<{ ok: boolean; text: string } | null>(null);
  const [plugin, setPlugin] = useState(false);
  useEffect(() => {
    if (!wants) return;
    let live = true;
    void hooksInstalled().then((has) => live && setMissing(!has));
    return () => {
      live = false;
    };
  }, [wants]);
  if (!wants || (!missing && !said)) return null;
  const setUp = async () => {
    setBusy(true);
    const o = await setUpClaude();
    setBusy(false);
    if (o.ok) setMissing(false);
    setSaid({ ok: o.ok, text: o.ok ? `${o.said} Start Claude again here to use them.` : o.said });
  };
  return (
    <div class="hooks-chip" role="status" data-hooks-chip={said ? (said.ok ? "done" : "failed") : "offer"} onPointerDown={(e) => e.stopPropagation()}>
      <span>{said ? said.text : "Its questions could be cards here and on your phone."}</span>
      {!said && (
        <button class="primary" disabled={busy} onClick={() => void setUp()} data-hooks-chip-setup>
          {busy ? "Setting up…" : "Set up"}
        </button>
      )}
      {!said && (
        <button class="link" aria-expanded={plugin} onClick={() => setPlugin(!plugin)} data-hooks-chip-plugin>
          Or the plugin
        </button>
      )}
      {said && !said.ok && (
        <button class="link" onClick={() => openGettingStarted("agents", client)}>
          Getting started…
        </button>
      )}
      <button
        class="link"
        aria-label="Hide"
        title="Hide"
        onClick={() => {
          putAway.add(id);
          setSaid(null);
          setMissing(false);
        }}
        data-hooks-chip-hide
      >
        ✕
      </button>
      {plugin && !said && <ClaudePlugin lead="Claude Code's plugin brings the hooks and Arugula's tools. In Claude Code, then start it again here:" />}
    </div>
  );
}
