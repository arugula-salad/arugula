// Getting started (#110): what isn't on the first screen, one step at a
// time: welcome, the phone (Tailscale), the cloud (Arugula control),
// agents, and Things to try (buttons that each do one thing here, each
// marked done once used). Opens by itself once per account on control's
// page, once per browser on a daemon's own (the seen store, shared/seen.ts),
// then from the account menu (top right) and the phone's sheet, and says
// so when it closes.
//
// The steps that used to be commands to copy are buttons: the daemon runs
// them for its owner (`/api/setup`, crates/daemon/src/setup.rs) and says
// when one needs something only a person can do (a one-time sudo, a switch
// in Tailscale's admin console), with the command or the link. Where the
// daemon can't be asked (a page through control, a guest), each step falls
// back to the command.
//
// #335: agents need Claude Code's (or Codex's) ACP adapter, and Claude Code
// needs Arugula's MCP server to start its helpers as panes, and its hooks
// (`arugula hooks install`) to put a terminal Claude's questions and
// permission prompts on cards. The Agents step says each adapter's state
// next to Start an agent, with Install (in a pane) or the npm line when it
// can't, and checks the MCP server and the hooks; "Use Claude Code with
// Arugula" does all three in one click (`POST /api/setup/agents/claude`),
// then says what changed, naming the settings file. Where Claude Code is on the machine and isn't
// set up, the first screen offers that click too. Beside the hooks, it
// offers Claude Code's plugin, which brings the tools and the hooks at
// once, as its two `/plugin` lines (./claude-plugin).

import { opPath } from "../ops";
import { directory } from "../hosts";
import qrcode from "qrcode-generator";
import { useEffect, useRef, useState } from "preact/hooks";
import { dialogUp, subscribeDialogs, useModal } from "../../shared/ui/modal";
import type { Client } from "../client";
import { CopyText } from "../../shared/ui/copy";
import { notifyBlocker, pushNow, serveCommand, subscribePush, turnOnPush } from "./notify";
import { markSeen, seen, seenReady, subscribeSeen } from "../../shared/seen";
import { currentAccount } from "./app-bar";
import { keyHint } from "./keys";
import { openPanel } from "../trust/common";
import { shareSession } from "./people";
import { openSwarm } from "../swarm/route";
import { openPalette } from "./palette";
import { openPort, openPr } from "../blocks";
import { startAgent } from "./agent-dialog";
import { AgentCanCard } from "./agent-can";
import { ClaudePlugin } from "./claude-plugin";
import type { ControlState, HostInfo } from "../../shared/proto";
import { placeOf, refreshControlState } from "./control-state";
import { desktopApp } from "../../shared/desktop";
import { countView } from "../../shared/telemetry";
import { AdapterHelp, adapterLine, adapterReady, installAdapter, type Adapter } from "./adapter";

const DOCS = "https://docs.arugula.io";
/** Arugula cloud, unless the daemon was started with `--control` (#207). */
const CONTROL = "https://control.arugula.io";
/** In the seen store: it opened by itself once. */
const SEEN = "getting-started";
/** Where a browser remembered that before the seen store. */
const OLD_KEY = "arugula.getting-started";

/** What `/api/host` says about this daemon (the parts for this panel). */
type HostSays = Pick<HostInfo, "name" | "tailnet_url" | "tailnet_seen" | "control" | "team"> & {
  /** #325: joined, not joined, or dropped by control (sent beside `HostInfo`). */
  control_state?: ControlState;
};

/** `GET /api/setup`. */
interface Setup {
  tailscale: {
    state: "missing" | "stopped" | "needs-login" | "running";
    host?: string;
    https: boolean;
    serving: boolean;
    url?: string;
    seen: boolean;
  };
  control: {
    joined?: string;
    team?: string;
    /** `code` is the whole code: with the join secret after a dot (#760). */
    pending?: { code: string; approve: string; expires_ms: number };
    /** Approved by an older control's page (no join secret, #760): the
     * account's fingerprint, to check before it's saved. */
    confirm?: { account: string; approver: string; place: string };
    error?: string;
    /** #330: control said this machine's old key was removed, so the join
     * waiting has a new key. */
    removed?: { said: string; at?: number; by?: string; old_key: string; kept: string; new_key: string };
    /** #329: a join `arugulad join` started here, which this one waits for. */
    elsewhere?: { by: string; code?: string; approve?: string; expires_ms: number };
    url: string;
    /** #325: as `/api/host`'s `control_state` (absent: an older daemon). */
    state?: ControlState;
  };
  /** `hooks`: Arugula's hooks are in Claude Code's settings (`arugula
   * hooks status`; absent from an older daemon). */
  claude: { installed: boolean; tools: boolean; hooks?: boolean };
  /** #335: each adapter, with `found` (its agent's CLI is here). Absent
   * from an older daemon. */
  adapters?: Adapter[];
  /** #883: how a new terminal runs the CLI: `arugula`, or its full path
   * when the login shell doesn't find it; `added`: the startup files
   * Arugula put `~/.local/bin` on PATH in. Absent from an older daemon. */
  cli?: { command: string; added?: string[] };
}

/** `GET /api/setup?part=agents`: what the welcome offer and the Agents
 * step read, without waiting for Tailscale. */
type AgentsSetup = Pick<Setup, "claude" | "adapters" | "cli">;

/** What a button did. */
interface Outcome {
  ok: boolean;
  error?: string;
  fix?: string;
  link?: { label: string; url: string };
  /** What changed, a line each (#335). */
  done?: string[];
}

const STEPS = [
  { id: "welcome", label: "Welcome" },
  { id: "phone", label: "Phone" },
  { id: "cloud", label: "Cloud" },
  { id: "agents", label: "Agents" },
  { id: "try", label: "Things to try" },
] as const;
type StepId = (typeof STEPS)[number]["id"];

/** Older names for steps, from callers that open it at one. */
export type Section = StepId | "control" | "ready";

let shown: { client: Client | null; section?: Section } | null = null;
const listeners = new Set<() => void>();
const changed = () => listeners.forEach((fn) => fn());
let lastClient: Client | null = null;
/** Set when it closes, for the "it's in the menu" note. */
let closedOnce = false;

export function openGettingStarted(section?: Section, client?: Client) {
  countView("getting-started");
  shown = { client: client ?? lastClient, section };
  changed();
}

/** It opened by itself before: in the seen store (true until it has
 * loaded), or in this browser's old key. */
function greeted(): boolean {
  if (seen(SEEN)) return true;
  try {
    return localStorage.getItem(OLD_KEY) !== null;
  } catch {
    // No storage (a private window, blocked): this browser's seen store
    // forgets, so don't greet every time; the account's remembers.
    return !currentAccount();
  }
}

/** Getting started opened by itself on this page load. */
let greetedNow = false;

/** Getting started opens by itself on this load (or did): What's new
 * stays shut then, so a first visit gets one panel, not two. */
export const greetingDue = (client: Client) => greetedNow || (!client.state?.roles && !greeted());

/** Open it the first time: once per account on control's page, once per
 * browser on a daemon's own. Not for a guest, and not under automation
 * (tests expect a clean first screen; welcome.spec.ts turns it back on). */
export function useFirstRun(client: Client) {
  lastClient = client;
  const ready = !!client.state && client.state.sessions.length > 0;
  // The seen store loads after the page does.
  const [loaded, setLoaded] = useState(seenReady());
  useEffect(() => subscribeSeen(() => setLoaded(seenReady())), []);
  // Arugula #280: not while a dialog is up, such as the one a link opened
  // control's page for (the app's sign-in, a machine's join): it waits
  // until that's answered. It used to open under it, so the person
  // answered the link's dialog and found another behind it. On control's
  // page the link's dialog is up before the greeting can be: both wait
  // for the account, and the greeting also for the account's prefs.
  // Asked as it's about to open, and again whenever a dialog opens or
  // closes (an answer kept from when the dialog opened could be stale: it
  // isn't always in the page yet as it opens).
  const [dialogs, setDialogs] = useState(0);
  useEffect(() => subscribeDialogs(() => setDialogs((n) => n + 1)), []);
  useEffect(() => {
    if (!ready || !loaded || dialogUp() || client.state?.roles || navigator.webdriver || greeted()) return;
    markSeen(SEEN);
    greetedNow = true;
    openGettingStarted(undefined, client);
  }, [client, ready, loaded, dialogs]);
}

export function GettingStartedLayer() {
  const [, setTick] = useState(0);
  useEffect(() => {
    const fn = () => setTick((t) => t + 1);
    listeners.add(fn);
    return () => void listeners.delete(fn);
  }, []);
  if (shown)
    return (
      <GettingStarted
        client={shown.client}
        section={shown.section}
        close={() => {
          if (seenReady()) markSeen(SEEN);
          shown = null;
          closedOnce = true;
          changed();
          setTimeout(() => {
            closedOnce = false;
            changed();
          }, 6000);
        }}
      />
    );
  // Where it went: it doesn't just vanish.
  return closedOnce ? (
    <div class="start-toast" role="status" data-start-toast>
      Getting started is in the account menu, top right, any time.
    </div>
  ) : null;
}

async function post(path: string, body?: unknown): Promise<Outcome & Record<string, unknown>> {
  try {
    const r = await fetch(path, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: body === undefined ? "{}" : JSON.stringify(body),
    });
    if (!r.ok) return { ok: false, error: (await r.json().catch(() => null))?.error ?? `HTTP ${r.status}` };
    return await r.json();
  } catch (e) {
    return { ok: false, error: String(e) };
  }
}

function stepFor(section?: Section): number {
  const id = section === "control" ? "cloud" : section === "ready" ? "try" : section;
  const i = STEPS.findIndex((s) => s.id === id);
  return i < 0 ? 0 : i;
}

function GettingStarted({ client, section, close }: { client: Client | null; section?: Section; close: () => void }) {
  const ref = useRef<HTMLDivElement>(null);
  // UI audit #150: takes the focus, keeps it, and Escape closes it.
  useModal(ref, close);
  const [step, setStep] = useState(stepFor(section));
  const [host, setHost] = useState<HostSays | null>(null);
  const [setup, setSetup] = useState<Setup | null>(null);
  // The daemon can't run the steps here (through control, or not its
  // owner): show the commands instead.
  const [manual, setManual] = useState(false);
  const [, setTick] = useState(0);
  useEffect(() => subscribePush(() => setTick((t) => t + 1)), []);
  useEffect(() => setStep(stepFor(section)), [section]);

  const refresh = async () => {
    try {
      const r = await fetch(opPath("setup.get"));
      if (!r.ok) throw new Error(String(r.status));
      setSetup((await r.json()) as Setup);
    } catch {
      setManual(true);
    }
  };
  // The cloud step's part comes back first (the rest asks Tailscale and
  // Claude Code): which control the button joins, at once (#207).
  const [early, setEarly] = useState<Setup["control"] | null>(null);
  // #335: the agents' part too, so their step doesn't wait on Tailscale
  // (whose `status` can take seconds when it's stopped). Once it's here,
  // it's what that step and the offer read, refreshed after their clicks.
  const [agentsPart, setAgentsPart] = useState<AgentsSetup | null>(null);
  const refreshAgents = async () => {
    const r = await fetch(`${opPath("setup.get")}?part=agents`).catch(() => null);
    if (r?.ok) setAgentsPart((await r.json()) as AgentsSetup);
  };
  const agents: AgentsSetup | null = agentsPart ?? setup;
  useEffect(() => {
    void refresh();
    void refreshAgents();
    fetch(`${opPath("setup.get")}?part=control`)
      .then((r) => (r.ok ? (r.json() as Promise<Pick<Setup, "control">>) : null))
      .then((r) => r && setEarly(r.control))
      .catch(() => {});
    fetch(opPath("host.get"))
      .then((r) => (r.ok ? (r.json() as Promise<HostSays>) : null))
      .then(setHost)
      .catch(() => {});
  }, []);

  // While a join waits for approval (or was just confirmed and the daemon
  // is picking it up), look for the answer.
  const [confirmed, setConfirmed] = useState(false);
  const waiting = !!setup?.control.pending || (confirmed && !setup?.control.joined);
  useEffect(() => {
    if (!waiting) return;
    const t = setInterval(async () => {
      const r = await fetch(`${opPath("setup.get")}?part=control`).catch(() => null);
      const got = r?.ok ? ((await r.json()) as Pick<Setup, "control">) : null;
      if (got) setSetup((s) => (s ? { ...s, control: got.control } : s));
    }, 2000);
    return () => clearInterval(t);
  }, [waiting]);

  const done: Record<StepId, boolean> = {
    welcome: true,
    phone: !!(setup?.tailscale.serving || host?.tailnet_seen),
    cloud: !!(setup?.control.joined || (host?.control && host.control_state?.state !== "dropped")),
    agents: claudeReady(agents),
    try: false,
  };
  const id = STEPS[step].id;
  const last = step === STEPS.length - 1;
  const go = (i: number) => setStep(Math.max(0, Math.min(STEPS.length - 1, i)));
  // Through control the daemon's host.get isn't this page's: the machine
  // shown is the one (UI audit #174: "this machine" from the Docs page).
  const name = host?.name ?? (directory.current || "this machine");

  return (
    <div class="prompt-backdrop start-backdrop" role="dialog" aria-modal="true" aria-label="Getting started" onPointerDown={(e) => e.target === e.currentTarget && close()}>
      <div
        ref={ref}
        class="start"
        data-getting-started
        data-step={id}
        onKeyDown={(e) => {
          if ((e.target as HTMLElement).closest("input, textarea")) return;
          if (e.key === "ArrowRight") go(step + 1);
          if (e.key === "ArrowLeft") go(step - 1);
        }}
        tabIndex={-1}
      >
        <header class="start-head">
          <div class="start-brand">
            Arugula <span>setup</span>
          </div>
          <button class="start-x" aria-label="Close" onClick={close}>
            ✕
          </button>
        </header>

        <ol class="start-rail" aria-label="Steps">
          {STEPS.map((s, i) => (
            <li key={s.id}>
              <button
                class={`start-seg${i === step ? " now" : ""}${done[s.id] && s.id !== "welcome" ? " done" : ""}`}
                aria-current={i === step ? "step" : undefined}
                onClick={() => go(i)}
                data-start-seg={s.id}
              >
                <b>{String(i + 1).padStart(2, "0")}</b> <span>{s.label}</span>
              </button>
            </li>
          ))}
        </ol>

        <div class="start-body">
          <div class="start-kicker" data-start-progress>
            Step {step + 1} / {STEPS.length} · {STEPS[step].label}
          </div>
          {id === "welcome" && <Welcome name={name} client={client} setup={agents} manual={manual} refresh={refreshAgents} />}
          {id === "phone" && <Phone name={name} setup={setup} host={host} manual={manual} refresh={refresh} />}
          {id === "cloud" && (
            <Cloud
              name={name}
              client={client}
              setup={setup}
              early={early}
              host={host}
              manual={manual}
              refresh={refresh}
              setSetup={setSetup}
              onConfirmed={() => setConfirmed(true)}
              close={close}
            />
          )}
          {id === "agents" && <Agents client={client} setup={agents} manual={manual} refresh={refreshAgents} close={close} />}
          {id === "try" && <Try client={client} done={done} go={go} close={close} />}
        </div>

        <footer class="start-foot">
          <button class="secondary" disabled={step === 0} onClick={() => go(step - 1)}>
            ‹ Back
          </button>
          <span class="start-hint">Always in the account menu: Getting started</span>
          {last ? (
            <button class="primary" onClick={close}>
              Done
            </button>
          ) : (
            <button class="primary" onClick={() => go(step + 1)} data-start-next>
              {step === 0 ? "Set it up ›" : done[id] ? "Next ›" : "Skip for now ›"}
            </button>
          )}
        </footer>
      </div>
    </div>
  );
}

// ---- steps

/** Claude Code works here: its adapter's at the pin, and it has the MCP
 * server and the hooks. (An older daemon doesn't say the adapter's or
 * the hooks'.) */
function claudeReady(setup: AgentsSetup | null): boolean {
  const a = setup?.adapters?.find((x) => x.kind === "claude");
  return !!setup?.claude.tools && setup.claude.hooks !== false && (!a || adapterReady(a));
}

function Welcome({ name, client, setup, manual, refresh }: { name: string; client: Client | null; setup: AgentsSetup | null; manual: boolean; refresh: () => Promise<void> }) {
  const panes = client?.state?.panes.length ?? 0;
  // #335: Claude Code is here and isn't set up: offer it without being asked.
  const offer = !manual && !!setup?.claude.installed && !claudeReady(setup);
  // #883: a new terminal may not find `arugula` by name.
  const cli = setup?.cli?.command ?? "arugula";
  const added = setup?.cli?.added ?? [];
  return (
    <section class="start-step">
      <h2>Terminals that outlive their windows</h2>
      <p class="start-lede">
        <b>{name}</b> keeps {panes === 1 ? "a pane" : `${panes} panes`} running whether or not a window is open. Close this tab, open it on your
        phone, come back tomorrow: they're where you left them.
      </p>
      <ul class="start-cards">
        <li>
          <span class="start-glyph">◎</span>
          <div>
            <h3>Right-click anything</h3>
            <p>Tabs, panes and the tab bar have menus. That's where everything is.</p>
          </div>
        </li>
        <li>
          <span class="start-glyph">⇲</span>
          <div>
            <h3>Drag to split</h3>
            <p>Drop a tab on a pane's edge to split it there.</p>
          </div>
        </li>
        <li>
          <span class="start-glyph">⌘</span>
          <div>
            <h3>Jump anywhere</h3>
            <p>
              <kbd>Ctrl</kbd> <kbd>Shift</kbd> <kbd>P</kbd> finds a pane, a tab or a command.
            </p>
          </div>
        </li>
        <li>
          <span class="start-glyph">›_</span>
          <div>
            <h3>From a terminal</h3>
            <p>
              Run, watch and script panes from a shell, over ssh too: <CopyText inline text={`${cli} ls`} />
            </p>
            {added.length > 0 && (
              <p class="start-dim" data-start-cli-added>
                Arugula put ~/.local/bin on your PATH in {added.join(" and ")}. In a terminal that was already open: <CopyText inline text="~/.local/bin/arugula ls" />
              </p>
            )}
          </div>
        </li>
      </ul>
      {offer && (
        <div class="start-offer" data-start-offer>
          <p>
            <b>Claude Code is on this machine.</b> One click lets it run here as agent panes, start its helpers as panes you can watch, and put its questions
            on cards: it installs Claude Code's adapter, and adds Arugula's MCP server and hooks.
          </p>
          <UseClaude setup={setup} refresh={refresh} />
        </div>
      )}
      <p class="start-dim">Next: your phone, the cloud and agents. Each takes a click, and you can skip any of them.</p>
    </section>
  );
}

/** One line of a checklist: done, waiting, wrong, or not yet. */
function Check({ state, children }: { state: "done" | "wait" | "fail" | "todo"; children: preact.ComponentChildren }) {
  return (
    <li class={`start-check ${state}`} data-check={state}>
      <i />
      <span>{children}</span>
    </li>
  );
}

function Said({ outcome }: { outcome: Outcome | null }) {
  if (!outcome || outcome.ok) return null;
  return (
    <div class="start-said" data-start-error>
      <p>{outcome.error}</p>
      {outcome.fix && (
        <>
          <p class="start-dim">Run this once on the machine, then try again:</p>
          <CopyText text={outcome.fix} data-start-fix />
        </>
      )}
      {outcome.link && (
        <a class="secondary" href={outcome.link.url} target="_blank" rel="noreferrer" data-start-link>
          {outcome.link.label} ↗
        </a>
      )}
    </div>
  );
}

function Qr({ text }: { text: string }) {
  const qr = qrcode(0, "M");
  qr.addData(text);
  qr.make();
  const n = qr.getModuleCount();
  let d = "";
  for (let r = 0; r < n; r++) for (let c = 0; c < n; c++) if (qr.isDark(r, c)) d += `M${c} ${r}h1v1h-1z`;
  return (
    <svg class="start-qr" viewBox={`-2 -2 ${n + 4} ${n + 4}`} role="img" aria-label={`QR code for ${text}`} data-start-qr>
      <rect x="-2" y="-2" width={n + 4} height={n + 4} />
      <path d={d} />
    </svg>
  );
}

function Phone({ name, setup, host, manual, refresh }: { name: string; setup: Setup | null; host: HostSays | null; manual: boolean; refresh: () => Promise<void> }) {
  const [busy, setBusy] = useState(false);
  const [outcome, setOutcome] = useState<Outcome | null>(null);
  const ts = setup?.tailscale;
  const url = ts?.url ?? host?.tailnet_url;
  const blocked = notifyBlocker();
  const serve = async () => {
    setBusy(true);
    const o = await post(opPath("setup.tailscale"));
    setOutcome(o);
    await refresh();
    setBusy(false);
  };
  return (
    <section class="start-step">
      <h2>Reach {name} from your phone</h2>
      <p class="start-lede">Tailscale puts this machine on your private network, with HTTPS. Your phone opens it like an app, and gets notified when a pane needs you.</p>
      {manual ? (
        <>
          <p>On this machine:</p>
          <CopyText text={serveCommand()} data-serve-command />
        </>
      ) : (
        <>
          <ul class="start-checks">
            <Check state={!ts ? "todo" : ts.state === "running" ? "done" : ts.state === "missing" ? "fail" : "wait"}>
              {!ts ? "Tailscale" : ts.state === "running" ? `On the tailnet as ${ts.host ?? name}` : ts.state === "missing" ? "Tailscale isn't installed" : ts.state === "needs-login" ? "Tailscale needs you to sign in" : "Tailscale isn't running"}
            </Check>
            <Check state={ts?.https ? "done" : "todo"}>HTTPS certificates for the tailnet</Check>
            <Check state={ts?.serving ? "done" : "todo"}>Served at its tailnet address</Check>
            <Check state={ts?.seen || host?.tailnet_seen ? "done" : "todo"}>Opened from another device</Check>
          </ul>
          {!ts?.serving && (
            <button class="primary big" disabled={busy || !setup} onClick={serve} data-start-serve>
              {busy ? "Putting it on the tailnet…" : "Put it on my tailnet"}
            </button>
          )}
          <Said outcome={outcome} />
          {ts?.state === "missing" && (
            <p class="start-dim" data-start-no-tailscale>
              No Tailscale? The next step, the cloud, reaches this machine from your phone too.
            </p>
          )}
        </>
      )}
      {url && (ts?.serving ?? true) && (
        <div class="start-reach">
          <Qr text={url} />
          <div>
            <p>Scan it, or open it on the phone (signed in to Tailscale as you):</p>
            <CopyText text={url} share data-tailnet-url />
            <p class="start-dim">
              Then add it to the Home Screen, open it from there, and turn on <em>Notify this device</em> in its menu (☰).
              {blocked && pushNow() !== "insecure" && <> This device: {blocked}.</>}
            </p>
          </div>
        </div>
      )}
    </section>
  );
}

function hostOf(url: string): string {
  try {
    return new URL(url).host;
  } catch {
    return url;
  }
}

/** A join code to approve, big: with a join secret (#760), the secret
 * goes on a line of its own after the code. The link carries both; typed,
 * it's all of it, dot included. */
function WholeCode({ code }: { code: string }) {
  const dot = code.indexOf(".");
  return (
    <div class="start-code-big" data-start-code>
      {dot < 0 ? (
        code
      ) : (
        <>
          {code.slice(0, dot)}
          <span class="start-code-secret">{code.slice(dot)}</span>
        </>
      )}
    </div>
  );
}

/** #326: joining this machine is one approval, of its code, on a device
 * already in the account, where the account or team is picked. Approving
 * a browser (or the app's window) as a device is another, for reaching
 * machines from it, and joining doesn't need it: the step says so. */
function Cloud({
  name,
  client,
  setup,
  early,
  host,
  manual,
  refresh,
  setSetup,
  onConfirmed,
  close,
}: {
  name: string;
  client: Client | null;
  setup: Setup | null;
  early: Setup["control"] | null;
  host: HostSays | null;
  manual: boolean;
  refresh: () => Promise<void>;
  setSetup: (fn: (s: Setup | null) => Setup | null) => void;
  onConfirmed: () => void;
  close: () => void;
}) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // The control part comes back first (the rest waits on Tailscale): a
  // code waiting for approval shows at once.
  const c = setup?.control ?? early ?? undefined;
  // #325: control dropped this machine; what's saved here doesn't count.
  const was = c?.state ?? host?.control_state;
  const dropped = was?.state === "dropped" ? was : null;
  const joined = dropped ? undefined : (c?.joined ?? host?.control);
  const team = c?.team ?? host?.team;
  // The control this daemon joins by default: its own, if it has one.
  const control = c?.url || early?.url || CONTROL;
  const ours = control === CONTROL;
  const connect = async () => {
    setBusy(true);
    setError(null);
    const r = await post(opPath("setup.control"), { url: control });
    if (r.error) setError(r.error);
    else if (r.pending) setSetup((s) => (s ? { ...s, control: { ...s.control, pending: r.pending as NonNullable<Setup["control"]["pending"]> } } : s));
    await refresh();
    setBusy(false);
  };
  // The person compared the account's fingerprints (an older control, with
  // no join secret, #760): keep the join, or drop it.
  const confirm = async (same: boolean) => {
    setBusy(true);
    const r = await post(opPath("setup.control_confirm"), { same });
    const got = (r as { control?: Setup["control"] }).control;
    if (got) setSetup((s) => (s ? { ...s, control: got } : s));
    if (same) {
      onConfirmed();
      // The banner and the host menu's line (#325) catch up.
      refreshControlState();
    }
    setBusy(false);
  };
  // Why this machine is out, and what happened since (#325, #330), above
  // whatever comes next: the code to approve, or the button.
  const removed = c?.removed;
  const droppedNote = dropped && (
    <div class="start-said" data-start-dropped>
      <p>
        This machine is no longer in {placeOf(dropped)}: control dropped it (it left, or was removed).
        {dropped.said ? ` Control says: ${dropped.said}.` : ""}{" "}
        {removed
          ? `Its key can't come back, so it made a new one (${removed.new_key}) and asks to join again; the old one is kept at ${removed.kept}.`
          : "Join again to put it back; the approver picks the account or team."}
      </p>
    </div>
  );
  return (
    <section class="start-step">
      <h2 data-start-cloud-title>{joined ? "Use it from anywhere" : dropped ? `Put ${name} back in your account or team` : `Add ${name} to your account or team`}</h2>
      <p class="start-lede">
        Arugula cloud reaches this machine from any browser, with no tailnet, and lets your team in on the sessions you share. Your terminals stay here:
        the cloud passes encrypted traffic, and only your devices hold the keys.
      </p>
      {manual ? (
        <>
          <p>On this machine:</p>
          <CopyText text={`arugulad join ${control}`} data-join-command />
        </>
      ) : joined ? (
        <>
          <ul class="start-checks">
            <Check state="done">
              <span data-start-joined>Joined{team ? ` to the team ${team}` : " to your account"}</span> ·{" "}
              <a href={joined} target="_blank" rel="noreferrer">
                open the cloud ↗
              </a>
            </Check>
          </ul>
          <Together joined={joined} client={client} close={close} />
          <p class="start-dim" data-start-next-device>
            {desktopApp()
              ? "Next, if you like: sign the app in and approve it as a device, so this window reaches your other machines too. It's optional: this machine is in already."
              : "To reach it from another browser or phone, sign in to the cloud there. Each one is approved once as a device; this machine doesn't need that."}
          </p>
        </>
      ) : c?.confirm ? (
        <div class="start-code" data-start-confirm>
          <div class="start-kicker">Approved on {c.confirm.approver || "your device"}. Is this your account?</div>
          <div class="start-code-big start-fp" data-start-account={c.confirm.account}>
            {c.confirm.account}
          </div>
          <p class="start-dim">
            The device you approved on shows your account's fingerprint in the approval and under Devices and machines. Check they're the same before this
            machine trusts the account.
          </p>
          <div class="start-confirm">
            <button class="primary" disabled={busy} onClick={() => void confirm(true)} data-start-same>
              They match
            </button>
            <button class="secondary" disabled={busy} onClick={() => void confirm(false)} data-start-different>
              They don't
            </button>
          </div>
        </div>
      ) : c?.pending ? (
        <div class="start-code" data-start-pending>
          {droppedNote}
          <div class="start-kicker">Approve this code on a device you use</div>
          <WholeCode code={c.pending.code} />
          <a class="primary big" href={c.pending.approve} target="_blank" rel="noreferrer" data-start-approve>
            Approve in Arugula cloud ↗
          </a>
          <p class="start-dim">
            <span class="start-pulse" /> Waiting for the approval. Open the link (or type the whole code) on a browser or phone you already use with
            Arugula; that's where you pick where {name} goes: your account, or a team you own. New here? It signs you up first (a passkey or GitHub).
          </p>
          <p class="start-dim" data-start-one-approval>
            That's the only approval {name} needs. A browser that isn't one of your devices yet asks to be approved itself; that's separate, and only for
            reaching your machines from it. A device you already use approves both at once.
          </p>
        </div>
      ) : c?.elsewhere ? (
        <div class="start-code" data-start-elsewhere>
          {droppedNote}
          <div class="start-kicker">{c.elsewhere.by} is joining this machine</div>
          {c.elsewhere.code && <WholeCode code={c.elsewhere.code} />}
          {c.elsewhere.approve && (
            <a class="primary big" href={c.elsewhere.approve} target="_blank" rel="noreferrer" data-start-approve>
              Approve in Arugula cloud ↗
            </a>
          )}
          <p class="start-dim">One join at a time: approve this one, or stop it where it runs and try again here.</p>
        </div>
      ) : (
        <>
          {droppedNote}
          <p class="start-dim" data-start-join-how>
            {name} shows a code, and you approve it on a device you use, picking your account or a team there: that one approval joins it.
          </p>
          <button class="primary big" disabled={busy || !setup} onClick={connect} data-start-connect>
            {busy ? "Asking the cloud…" : dropped ? `Join ${hostOf(control)} again` : ours ? "Connect to Arugula cloud" : `Connect to ${hostOf(control)}`}
          </button>
          {(error ?? c?.error) && (
            <div class="start-said" data-start-error>
              <p>{error ?? c?.error}</p>
            </div>
          )}
        </>
      )}
    </section>
  );
}

/** #335: "Use Claude Code with Arugula": its adapter (installed, or
 * updated to the pin), Arugula's MCP server and its hooks, then what
 * changed. */
function UseClaude({ setup, refresh }: { setup: AgentsSetup | null; refresh: () => Promise<void> }) {
  const [busy, setBusy] = useState(false);
  const [outcome, setOutcome] = useState<Outcome | null>(null);
  const go = async () => {
    setBusy(true);
    setOutcome(await post(opPath("setup.agent", { kind: "claude" })));
    await refresh();
    setBusy(false);
  };
  const done = outcome?.done ?? [];
  return (
    <>
      {!claudeReady(setup) && (
        <button class="primary" disabled={busy || !setup} onClick={go} data-start-claude>
          {busy ? "Setting it up…" : "Use Claude Code with Arugula"}
        </button>
      )}
      {done.length > 0 && (
        <ul class="start-checks" data-start-done>
          {done.map((d) => (
            <Check key={d} state="done">
              {d}
            </Check>
          ))}
        </ul>
      )}
      <Said outcome={outcome} />
    </>
  );
}

function adapterCheck(a: Adapter): "done" | "wait" | "fail" | "todo" {
  if (adapterReady(a)) return "done";
  return a.state === "no_node" ? "fail" : a.outdated ? "wait" : "todo";
}

function Agents({ client, setup, manual, refresh, close }: { client: Client | null; setup: AgentsSetup | null; manual: boolean; refresh: () => Promise<void>; close: () => void }) {
  const session = client?.session ?? null;
  const cl = setup?.claude;
  // Claude Code's always; another's where it's used here.
  const adapters = (setup?.adapters ?? []).filter((a) => a.kind === "claude" || a.found || a.state === "installed");
  return (
    <section class="start-step">
      <h2>Put agents to work</h2>
      <p class="start-lede">Claude and Codex run here as agent blocks. When one asks something, it's a card you answer from here or the phone.</p>
      <ul class="start-cards">
        <li>
          <span class="start-glyph agent">✦</span>
          <div>
            <h3>Start an agent</h3>
            <p>Give it a task in this session; watch it work in a pane of its own.</p>
            {!manual && adapters.length > 0 && (
              <ul class="start-checks" data-start-adapters>
                {adapters.map((a) => (
                  <Check key={a.kind} state={adapterCheck(a)}>
                    <span data-start-adapter={a.kind}>{adapterLine(a)}</span>
                  </Check>
                ))}
              </ul>
            )}
            {client &&
              !manual &&
              adapters.map((a) => (
                <AdapterHelp
                  key={a.kind}
                  client={client}
                  a={a}
                  then="start the agent once it's done."
                  install={() => {
                    close();
                    void installAdapter(client, a.kind, { session: session ?? undefined });
                  }}
                />
              ))}
            {client && session !== null && (
              <button
                class="secondary"
                data-start-agent
                onClick={() => {
                  close();
                  startAgent(client, { session, from: client.active() });
                }}
              >
                Start an agent…
              </button>
            )}
          </div>
        </li>
        <li>
          <span class="start-glyph agent">⚙</span>
          <div>
            <h3>Claude Code, with Arugula</h3>
            <p>
              Agent panes run it through its adapter. Arugula's MCP server lets it put builds, servers and its own helpers in panes you can watch and take
              over. Its hooks put a terminal Claude's questions and permission prompts on cards, here and on your phone.
            </p>
            {manual ? (
              <>
                <p class="start-dim">On this machine, all three at once:</p>
                <CopyText text="arugula setup claude" data-setup-command />
                <p class="start-dim">Or only the MCP server, or only the hooks:</p>
                <CopyText text="claude mcp add arugula -- arugula mcp" data-mcp-command />
                <CopyText text="arugula hooks install" data-hooks-command />
                <ClaudePlugin />
              </>
            ) : (
              <>
                <ul class="start-checks">
                  <Check state={cl?.tools ? "done" : "todo"}>
                    <span data-start-tools>
                      {cl?.tools ? "Claude Code has Arugula's tools" : cl && !cl.installed ? "Claude Code isn't installed" : "Arugula's MCP server: not added yet"}
                    </span>
                  </Check>
                  {cl?.hooks !== undefined && (
                    <Check state={cl.hooks ? "done" : "todo"}>
                      <span data-start-hooks>{cl.hooks ? "Questions on cards: Claude Code has Arugula's hooks" : "Questions on cards: Arugula's hooks aren't in Claude Code's settings yet"}</span>
                    </Check>
                  )}
                </ul>
                <UseClaude setup={setup} refresh={refresh} />
                {/* Where the hooks are offered, the plugin too (Discovery 10). */}
                {cl?.installed !== false && (!cl?.tools || cl.hooks === false) && <ClaudePlugin />}
              </>
            )}
            <p class="start-dim">
              <a href={`${DOCS}/cli/#mcp`} target="_blank" rel="noreferrer">
                Which tools to allow
              </a>{" "}
              ·{" "}
              <a href={`${DOCS}/cli/#claude-code-in-a-pane`} target="_blank" rel="noreferrer">
                Claude Code in a pane
              </a>
            </p>
          </div>
        </li>
      </ul>
      {client && !manual && <AgentCanCard client={client} />}
    </section>
  );
}

/** The Cloud step's, once joined: a team to share with, and a session to
 * share. Teams are the account's: its panel on control's page, else the
 * cloud's own page, where they're in the account menu. */
function Together({ joined, client, close }: { joined: string; client: Client | null; close: () => void }) {
  const session = client?.session ?? null;
  const owner = !!client?.state && !client.state.roles;
  return (
    <div class="start-together" data-start-together>
      <p>Share with people: make a team, then share a session with it, or with one person.</p>
      <div class="start-confirm">
        {currentAccount() ? (
          <button
            class="secondary"
            data-start-make-team
            onClick={() => {
              close();
              openPanel("teams")();
            }}
          >
            Make a team
          </button>
        ) : (
          <a class="secondary" href={joined} target="_blank" rel="noreferrer" title="Teams are in the cloud's account menu" data-start-make-team>
            Make a team ↗
          </a>
        )}
        {client && owner && session !== null && (
          <button
            class="secondary"
            data-start-share
            onClick={() => {
              close();
              shareSession(session);
            }}
          >
            Share a session
          </button>
        )}
      </div>
    </div>
  );
}

/** One of Things to try: what it does, why it can't here, and what to do. */
interface TryItem {
  id: string;
  label: string;
  what: string;
  /** Done already, whatever was clicked (push that's on). */
  already?: boolean;
  refusal?: string | null;
  /** Leave Getting started open (a browser's own prompt comes next). */
  stay?: boolean;
  run: () => void;
}

/** The seen store's name for a thing tried. */
const tried = (id: string) => `getting-started.try.${id}`;

/** The last step: a button for each thing, on this machine, each marked
 * done once used (in the seen store), and what's still to set up. */
function Try({ client, done, go, close }: { client: Client | null; done: Record<StepId, boolean>; go: (i: number) => void; close: () => void }) {
  const [, setTick] = useState(0);
  useEffect(() => subscribeSeen(() => setTick((t) => t + 1)), []);
  const owner = !!client?.state && !client.state.roles;
  const session = client?.session ?? null;
  const active = client?.active();
  const mine = active === undefined ? undefined : client?.machine(active);
  const blocked = notifyBlocker();
  const items: TryItem[] = [
    { id: "swarm", label: "Open the swarm", what: "Every pane on every machine at once, what needs you first", run: () => openSwarm() },
    {
      id: "agent",
      label: "Start an agent",
      what: "Give Claude or Codex a task in a pane of its own",
      refusal: session === null ? "No session to start it in" : null,
      run: () => client && session !== null && startAgent(client, { session, from: client.active() }),
    },
    {
      id: "port",
      label: "Open a port",
      what: "Preview an app running here, in a pane beside this one",
      refusal: !client?.showsBlocks() ? "This page can't show block sites" : null,
      run: () => client && void openPort(client, { split: active, host: mine?.id, local: !mine }),
    },
    {
      id: "pr",
      label: "Open a pull request",
      what: "Its checks, reviews and timeline, read with your gh login",
      refusal: !owner ? "Pull requests open with the machine owner's login" : session === null ? "No session to open it in" : null,
      run: () => client && session !== null && void openPr(client, { session }),
    },
    {
      id: "push",
      label: "Get push",
      what: "This device is told when a pane or an agent needs you",
      already: pushNow() === "on",
      refusal: blocked,
      stay: true,
      run: () => client && void turnOnPush(client),
    },
    {
      id: "search",
      label: "Open command search",
      what: `Every command, session, tab and pane: ${keyHint("palette") ?? "Ctrl+Shift+P"}`,
      run: () => client && openPalette(client),
    },
  ];
  const rows: [StepId, string, string][] = [
    ["phone", "On your phone", "Not yet: Tailscale"],
    ["cloud", "In Arugula cloud", "Not yet: connect"],
    ["agents", "Claude Code works with Arugula", "Not yet: set up Claude Code"],
  ];
  const todo = rows.filter(([id]) => !done[id]);
  return (
    <section class="start-step">
      <h2>Things to try</h2>
      <p class="start-lede">Each of these does the thing, here and now. Try them in any order.</p>
      <ul class="start-tries" data-start-tries>
        {items.map((t) => {
          const isDone = t.already || (seenReady() && seen(tried(t.id)));
          return (
            <li key={t.id} class={`start-try${isDone ? " done" : ""}`} data-start-try={t.id} data-done={isDone ? "" : undefined}>
              <button
                class="secondary"
                disabled={!client || !!t.refusal}
                title={t.refusal ?? undefined}
                onClick={() => {
                  markSeen(tried(t.id));
                  if (!t.stay) close();
                  t.run();
                }}
              >
                {isDone && <span class="start-try-mark" aria-label="done">✓ </span>}
                {t.label}
              </button>
              <span class="start-dim">{t.refusal ?? t.what}</span>
            </li>
          );
        })}
      </ul>
      {todo.length > 0 && (
        <ul class="start-checks" data-start-todo>
          {todo.map(([id, , no]) => (
            <Check key={id} state="todo">
              <button class="link" onClick={() => go(STEPS.findIndex((s) => s.id === id))}>
                {no} ›
              </button>
            </Check>
          ))}
        </ul>
      )}
      <p class="start-lede">
        Anything you skipped is a click away: this is <b>Getting started</b> in the account menu, top right (and the ☰ sheet on a phone), any time.
      </p>
    </section>
  );
}
