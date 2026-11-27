// Getting started (#110; Lift 7, #1487): one screen, Add a machine. This
// machine joins Arugula cloud in a click (Connect opens the cloud with its
// code, a signed-in person clicks Add there, picking their account or a
// team), and `arugulad join <control>` is there to copy, for this machine
// or any other. It doesn't open by itself
// (https://app.arugula.io/issues/iss_fa949b979b607ef8f0414cbe674ab33b): a
// first visit opens on the person's own work. It's in Go to…'s Help, the
// phone's sheet, a team chat's Join, and Join again when the cloud dropped
// this machine.
//
// Where Claude Code is on the machine and isn't set up (or only partly,
// #906), the screen offers "Use Claude Code with Arugula" under it, one
// click for its adapter, Arugula's MCP server and its hooks (`POST
// /api/setup/agents/claude`, #335). Opened at `agents` (the hooks' nudge,
// the hint's Set it up), that offer shows whatever the state. Lift 7 cut the
// Welcome, Agents and Things to try steps: adapters are the Start-an-agent
// dialog's and the agent block's, what an agent can do is that dialog's, and
// the hints say the rest at their moment.
//
// The phone over Tailscale is Settings' (This machine's Tailscale…, the
// machine menu's too): a dialog of its own (`openTailscale`).
//
// Where the daemon can't be asked (a page through control, a guest), the
// screen is the command alone.

import { opPath } from "../ops";
import { directory } from "../hosts";
import qrcode from "qrcode-generator";
import { useEffect, useRef, useState } from "preact/hooks";
import { Modal } from "../../shared/ui/modal";
import { Sheet } from "../../shared/kit/sheet";
import type { Client } from "../client";
import { CopyText } from "../../shared/ui/copy";
import { notifyBlocker, pushNow, serveCommand } from "./notify";
import { refreshControlState, placeOf } from "./control-state";
import { desktopApp } from "../../shared/desktop";
import { countView } from "../../shared/telemetry";
import { adapterReady, type Adapter } from "./adapter";
import type { ControlState, HostInfo } from "../../shared/proto";
import { ExternalIcon } from "../../shared/ui/icons";

const CONTROL = "https://control.arugula.io";

/** What `/api/host` says about this daemon (the parts for this panel). */
type HostSays = Pick<HostInfo, "name" | "tailnet_url" | "tailnet_seen" | "control" | "team" | "features"> & {
  /** #325: joined, not joined, or dropped by control (sent beside `HostInfo`). */
  control_state?: ControlState;
};

/** `GET /api/setup`. */
/** #906: Arugula's hooks are only partly in Claude Code's settings
 * (`arugula --json hooks status`'s `partial`, as the daemon passes it on). */
export interface HooksPartial {
  missing: string[];
  outdated: string[];
  /** Some of the read-only permission rules, not all. */
  rules: boolean;
  says: string;
}

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
    /** `code` is the whole code: with the join secret after a dot (#760).
     * `approve` opens control with it, where a signed-in person clicks Add. */
    pending?: { code: string; approve: string; expires_ms: number };
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
  /** `hooks_blocked` (#395): what stops Claude Code running them, said for a person.
   * `hooks_partial` (#906): only some of them (or of the read-only rules) are
   * there: which, and `says`, the sentence with the command that finishes it. */
  claude: { installed: boolean; tools: boolean; hooks?: boolean; hooks_blocked?: string; hooks_partial?: HooksPartial };
  /** #335: each adapter, with `found` (its agent's CLI is here). Absent
   * from an older daemon. */
  adapters?: Adapter[];
  /** #883: how a new terminal runs the CLI: `arugula`, or its full path
   * when the login shell doesn't find it; `added`: the startup files
   * Arugula put `~/.local/bin` on PATH in. Absent from an older daemon. */
  cli?: { command: string; added?: string[] };
  /** nvim is on the daemon's PATH, for Edit in nvim. Absent from an older
   * daemon. */
  nvim?: boolean;
}

/** `GET /api/setup?part=agents`: what the welcome offer and the Agents
 * step read, without waiting for Tailscale. */
type AgentsSetup = Pick<Setup, "claude" | "adapters" | "cli" | "nvim">;

/** What a button did. */
interface Outcome {
  ok: boolean;
  error?: string;
  fix?: string;
  link?: { label: string; url: string };
  /** What changed, a line each (#335). */
  done?: string[];
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

/** Where callers open it: every one is the same screen now; `agents`
 * shows the Claude Code offer whatever its state, `phone` is Tailscale's
 * dialog. */
export type Section = "welcome" | "cloud" | "agents" | "try" | "control" | "ready" | "phone";

let shown: { client: Client | null; section?: Section } | null = null;
/** The Tailscale dialog is open. */
let tailscale = false;
const listeners = new Set<() => void>();
const changed = () => listeners.forEach((fn) => fn());
let lastClient: Client | null = null;

export function openGettingStarted(section?: Section, client?: Client) {
  if (section === "phone") return openTailscale();
  countView("getting-started");
  shown = { client: client ?? lastClient, section };
  changed();
}

/** Settings' Tailscale: put this machine on the tailnet, for a phone. */
export function openTailscale() {
  tailscale = true;
  changed();
}

/** The machine the page shows, for a caller that opens it without one. */
export function gettingStartedFor(client: Client) {
  lastClient = client;
}

export function GettingStartedLayer() {
  const [, setTick] = useState(0);
  useEffect(() => {
    const fn = () => setTick((t) => t + 1);
    listeners.add(fn);
    return () => void listeners.delete(fn);
  }, []);
  const close = () => {
    shown = null;
    tailscale = false;
    changed();
  };
  if (tailscale) return <TailscaleDialog close={close} />;
  return shown ? <GettingStarted section={shown.section} close={close} /> : null;
}

function GettingStarted({ section, close }: { section?: Section; close: () => void }) {
  const [host, setHost] = useState<HostSays | null>(null);
  const [setup, setSetup] = useState<Setup | null>(null);
  // The daemon can't run the join here (through control, or not its
  // owner): the command alone.
  const [manual, setManual] = useState(false);
  const refresh = async () => {
    try {
      const r = await fetch(opPath("setup.get"));
      if (!r.ok) throw new Error(String(r.status));
      setSetup((await r.json()) as Setup);
    } catch {
      setManual(true);
    }
  };
  // The control part comes back first (the rest asks Tailscale and Claude
  // Code): which control the button joins, at once (#207); then Claude
  // Code's, so its offer doesn't wait on Tailscale (#335).
  const [early, setEarly] = useState<Setup["control"] | null>(null);
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
  // While a join waits for someone to click Add, look for the answer: the
  // daemon saves it as soon as control says (#1003).
  const waiting = !!setup?.control.pending;
  useEffect(() => {
    if (!waiting) return;
    const t = setInterval(async () => {
      const r = await fetch(`${opPath("setup.get")}?part=control`).catch(() => null);
      const got = r?.ok ? ((await r.json()) as Pick<Setup, "control">) : null;
      if (!got) return;
      setSetup((s) => (s ? { ...s, control: got.control } : s));
      // The banner and the host menu's line (#325) catch up.
      if (got.control.joined) refreshControlState();
    }, 2000);
    return () => clearInterval(t);
  }, [waiting]);
  // Through control the daemon's host.get isn't this page's: the machine
  // shown is the one (UI audit #174).
  const name = host?.name ?? (directory.current || "this machine");
  // Claude Code is here and isn't set up: offered without being asked; at
  // `agents`, whatever its state.
  // Once offered it stays, so its click can say what it did.
  const offered = useRef(false);
  if (!manual && agents && (section === "agents" || (agents.claude.installed && !claudeReady(agents)))) offered.current = true;
  const claude = offered.current;
  return (
    <Sheet title="Add a machine" onClose={close}>
      <div class="start" data-getting-started="">
        <Cloud name={name} setup={setup} early={early} host={host} manual={manual} refresh={refresh} setSetup={setSetup} />
        {claude && (
          <div class="start-offer" data-start-offer>
            <p>
              <b>Claude Code on {name}.</b> One click runs it here as agent panes, starts its helpers as panes you can watch, and puts its questions on
              cards.
            </p>
            <UseClaude setup={agents} refresh={refreshAgents} />
          </div>
        )}
      </div>
    </Sheet>
  );
}

/** Claude Code works here: its adapter's at the pin, and it has the MCP
 * server and the hooks. (An older daemon doesn't say the adapter's or
 * the hooks'.) */
function claudeReady(setup: AgentsSetup | null): boolean {
  const a = setup?.adapters?.find((x) => x.kind === "claude");
  return !!setup?.claude.tools && setup.claude.hooks !== false && !setup.claude.hooks_partial && (!a || adapterReady(a));
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
          {outcome.link.label} <ExternalIcon size="xs" />
        </a>
      )}
    </div>
  );
}

export function Qr({ text }: { text: string }) {
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
              No Tailscale? Arugula cloud reaches this machine from your phone too: Getting started connects it.
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

/** The Phone step, out of Getting started: Settings' way onto the tailnet.
 * What the daemon says of Tailscale can take seconds (`status` when it's
 * stopped), so it asks once, as it opens. */
function TailscaleDialog({ close }: { close: () => void }) {
  const [setup, setSetup] = useState<Setup | null>(null);
  const [host, setHost] = useState<HostSays | null>(null);
  const [manual, setManual] = useState(false);
  const refresh = async () => {
    try {
      const r = await fetch(opPath("setup.get"));
      if (!r.ok) throw new Error(String(r.status));
      setSetup((await r.json()) as Setup);
    } catch {
      setManual(true);
    }
  };
  useEffect(() => {
    void refresh();
    fetch(opPath("host.get"))
      .then((r) => (r.ok ? (r.json() as Promise<HostSays>) : null))
      .then(setHost)
      .catch(() => {});
  }, []);
  return (
    <Modal close={close} label="Tailscale" class="start-tailscale" root={{ "data-tailscale": "" }} buttons={<button class="primary" onClick={close}>Done</button>}>
      <Phone name={host?.name ?? (directory.current || "this machine")} setup={setup} host={host} manual={manual} refresh={refresh} />
    </Modal>
  );
}

function hostOf(url: string): string {
  try {
    return new URL(url).host;
  } catch {
    return url;
  }
}

/** A join code, big (`typed`: smaller, to type elsewhere): with a join secret (#760), the secret
 * goes on a line of its own after the code. The link carries both; typed,
 * it's all of it, dot included. */
function WholeCode({ code, typed = false }: { code: string; typed?: boolean }) {
  const dot = code.indexOf(".");
  return (
    <div class={typed ? "start-code-big start-code-typed" : "start-code-big"} data-start-code>
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

function Cloud({
  name,
  setup,
  early,
  host,
  manual,
  refresh,
  setSetup,
}: {
  name: string;
  setup: Setup | null;
  early: Setup["control"] | null;
  host: HostSays | null;
  manual: boolean;
  refresh: () => Promise<void>;
  setSetup: (fn: (s: Setup | null) => Setup | null) => void;
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
    // Nothing asked about typing: the join settles it, on for your account
    // and off for a team (Arugula issue iss_d8ce0c357cf0902c51dec39378ef7a1b).
    const r = await post(opPath("setup.control"), { url: control });
    if (r.error) setError(r.error);
    else if (r.pending) setSetup((s) => (s ? { ...s, control: { ...s.control, pending: r.pending as NonNullable<Setup["control"]["pending"]> } } : s));
    await refresh();
    setBusy(false);
  };
  // Why this machine is out, and what happened since (#325, #330), above
  // whatever comes next: the code to approve, or the button.
  const removed = c?.removed;
  const droppedNote = dropped && (
    <div class="start-said" data-start-dropped>
      <p>
        This machine is no longer in {placeOf(dropped)}: Arugula cloud dropped it (it left, or was removed).
        {dropped.said ? ` Arugula cloud says: ${dropped.said}.` : ""}{" "}
        {removed
          ? `Its key can't come back, so it made a new one (${removed.new_key}) and asks to join again; the old one is kept at ${removed.kept}.`
          : "Join again to put it back; you pick the account or team in Arugula cloud."}
      </p>
    </div>
  );
  return (
    <section class="start-step">
      <h2 data-start-cloud-title>{joined ? "Use it from anywhere" : dropped ? `Put ${name} back in your account or team` : `Add ${name} to your account or team`}</h2>
      <p class="start-lede">Arugula cloud reaches it from any browser or phone, and lets your team in on what you share.</p>
      <p class="start-dim" data-start-remote-input>
        In your account, you can type into {name}'s panes from your phone and other devices. In a team, people only watch until you run{" "}
        <code>arugulad sharing remote-input on</code> here; <code>off</code> makes everyone watch.
      </p>
      {manual ? null : joined ? (
        <>
          <ul class="start-checks">
            <Check state="done">
              <span data-start-joined>Joined{team ? ` to the team ${team}` : " to your account"}</span> ·{" "}
              <a href={joined} target="_blank" rel="noreferrer">
                open the cloud <ExternalIcon size="xs" />
              </a>
            </Check>
          </ul>
          <p class="start-dim" data-start-next-device>
            {desktopApp()
              ? "Next, if you like: sign the app in, so this window reaches your other machines too. It's optional: this machine is in already."
              : "To reach it from another browser or phone, sign in to Arugula cloud there."}
          </p>
        </>
      ) : c?.pending ? (
        <div class="start-code" data-start-pending>
          {droppedNote}
          <a class="primary big" href={c.pending.approve} target="_blank" rel="noreferrer" data-start-approve>
            Add it in Arugula cloud <ExternalIcon size="xs" />
          </a>
          <p class="start-dim">
            <span class="start-pulse" /> Waiting for you to click Add there, where you pick your account or a team. Not signed in there yet? It signs you
            in first, or up if you're new (a passkey or GitHub).
          </p>
          <p class="start-dim" data-start-code-elsewhere>
            Signed in on another computer or phone instead? Type this code under <i>Add a machine</i> there:
          </p>
          <WholeCode code={c.pending.code} typed />
        </div>
      ) : c?.elsewhere ? (
        <div class="start-code" data-start-elsewhere>
          {droppedNote}
          <div class="start-kicker">{c.elsewhere.by} is joining this machine</div>
          {c.elsewhere.code && <WholeCode code={c.elsewhere.code} />}
          {c.elsewhere.approve && (
            <a class="primary big" href={c.elsewhere.approve} target="_blank" rel="noreferrer" data-start-approve>
              Add it in Arugula cloud <ExternalIcon size="xs" />
            </a>
          )}
          <p class="start-dim">One join at a time: add this one, or stop it where it runs and try again here.</p>
        </div>
      ) : (
        <>
          {droppedNote}
          <p class="start-dim" data-start-join-how>
            Connect opens Arugula cloud with {name}'s code. Click Add there, picking your account or a team, and it's in.
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
      <div class="start-command" data-start-command>
        <p class="start-dim">{manual ? `On ${name}:` : "Or on any machine, this one or another:"}</p>
        <CopyText text={`arugulad join ${control}`} data-join-command />
      </div>
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
      {claudeReady(setup) && !outcome && (
        <ul class="start-checks">
          <Check state="done">
            <span data-start-claude-ready>Claude Code is set up</span>
          </Check>
        </ul>
      )}
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

/** A sentence the CLI wrote, its `commands` as code. */
export function withCode(text: string): preact.ComponentChildren {
  return text.split("`").map((t, i) => (i % 2 ? <code key={i}>{t}</code> : t));
}
