// The Chief of staff tab before there is one (Arugula #1341): what the
// chief of staff does, and Start, with a pick of the reader's machines that
// can run it: their own, online here and on a daemon that says it starts
// one (`features.chief_of_staff`), and the project's Cloud workspaces as one
// more entry, never the default. The default is the machine the page is on
// (a daemon's own page), if it can.
//
// Start calls `start_agent {recipe: "chief-of-staff", project}` on that
// machine, over the page's own connection to it (`POST /api/mcp/call`, as
// its owner, #2863), which starts the agent block and makes it the lead of
// the reader's staff room. Leading it needs the machine's `threads.read`
// over the reader's DMs (the staff room is one): without it the start goes
// on and the lead doesn't, so the tab asks for it first, on a card here,
// never in chat. Once started, the staff room is read from the hub and the
// tab is the room's, with its head (#1342). While the room's chief of staff
// is asleep, the same pick and start move it to another machine (StaffMove,
// #1453).

import { useState } from "preact/hooks";
import { chat } from "../shared/chat-store";
import { platformFetch } from "../shared/maintenance";
import { EmptyState } from "../shared/ui/block-states";
import { useSubscribe } from "../shared/ui/hooks";
import type { Fleet } from "../shell/fleet";
import { directory } from "../shell/hosts";
import { shortHost } from "../shell/swarm/model";
import { staffSpots, type StaffSpot } from "../shell/swarm/overview";
import { dmPowersMissing, type Grant } from "../platform/settings/grants";

/** What `start_agent` answers through `/api/mcp/call`: a tool's result. */
interface ToolResult {
  isError?: boolean;
  content?: { type: string; text?: string }[];
}

/** Start the chief of staff on `host` in `project`, or say why it didn't. */
async function startOn(fleet: Fleet, host: string, project: string): Promise<void> {
  const res = await fleet.request(host, "POST", "/api/mcp/call", {
    tool: "start_agent",
    arguments: { agent: "claude", recipe: "chief-of-staff", project, prompt: "" },
    client: "Overview",
  });
  const v = await res.json<ToolResult & { error?: string }>().catch(() => null);
  if (!res.ok || !v) throw new Error(v?.error ?? `${shortHost(host)} didn't start it (${res.status})`);
  if (v.isError) throw new Error((v.content ?? []).map((c) => c.text ?? "").join(" ").trim() || "it didn't start");
}

/** The reader's grants (`GET /api/grants`), over their control session. */
async function grants(): Promise<Grant[]> {
  const r = await platformFetch("/api/grants", { credentials: "same-origin" });
  if (!r.ok) throw new Error(`couldn't read your grants (${r.status})`);
  return ((await r.json()) as { grants: Grant[] }).grants;
}

type Step = { at: "idle" } | { at: "starting" } | { at: "grant"; spot: StaffSpot; powers: string[] } | { at: "failed"; why: string };

/** Starting the chief of staff from the tab: where it can run, the one
 * chosen, and how the start stands. `except` is a machine left out (the
 * one it's asleep on, #1453). */
function useStaffStart(project: string, fleet: Fleet, hostedIn: (id: string | undefined) => string | undefined, except?: string) {
  useSubscribe((fn) => fleet.subscribe(fn));
  useSubscribe((fn) => directory.subscribe(fn));
  const spots = staffSpots(fleet.list, { project, here: directory.home, features: (n) => fleet.clientOf(n)?.features, hostedIn });
  const can = spots.can.filter((s) => s.name !== except);
  const cannot = spots.cannot.filter((c) => c.name !== except);
  const pick = spots.pick !== except ? spots.pick : (can.find((s) => !s.cloud)?.name ?? null);
  const [chosen, setChosen] = useState<string | null>(null);
  const [step, setStep] = useState<Step>({ at: "idle" });
  const on = can.find((s) => s.name === (chosen ?? pick)) ?? null;

  async function start(spot: StaffSpot, ask = true) {
    setStep({ at: "starting" });
    try {
      // Its lead needs the machine's conversation powers over your DMs: asked here, on a card, before it starts.
      if (ask && spot.id) {
        const powers = dmPowersMissing(await grants(), spot.id);
        if (powers.length) return setStep({ at: "grant", spot, powers });
      }
      await startOn(fleet, spot.name, project);
      // The hub's staff room, with its lead: the tab becomes it, and its head
      // says "none" if the lead didn't take (its first turn tries again).
      await chat.staffRoom(project);
      setStep({ at: "idle" });
    } catch (e) {
      setStep({ at: "failed", why: (e as Error).message });
    }
  }

  async function allow(spot: StaffSpot, powers: string[]) {
    setStep({ at: "starting" });
    try {
      for (const power of powers) {
        const r = await platformFetch("/api/grants", { method: "POST", credentials: "same-origin", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ daemon: spot.id, power, scope: { kind: "account" } }) });
        if (!r.ok) throw new Error(((await r.json().catch(() => ({}))) as { error?: string }).error ?? `couldn't allow it (${r.status})`);
      }
    } catch (e) {
      return setStep({ at: "failed", why: (e as Error).message });
    }
    await start(spot, false);
  }

  return { can, cannot, on, step, setChosen, setStep, start, allow };
}
type StaffStarting = ReturnType<typeof useStaffStart>;

const label = (s: StaffSpot) => `${shortHost(s.name)}${s.here ? " (this computer)" : s.cloud ? " (Cloud workspace)" : ""}`;

/** The machine to run it on (a pick of several, or the one) and the button that starts it there. */
function StaffGo({ s, go }: { s: StaffStarting; go: string }) {
  const { can, on, step } = s;
  const busy = step.at === "starting";
  return (
    <div class="ov-staff-start-go">
      {can.length > 1 ? (
        <label>
          <span class="dim">Run it on</span>{" "}
          <select class="field" value={on?.name ?? ""} disabled={busy} data-ov-staff-machine onChange={(e) => s.setChosen(e.currentTarget.value || null)}>
            {!on && <option value="">Pick a machine</option>}
            {can.map((x) => (
              <option key={x.name} value={x.name}>
                {label(x)}
              </option>
            ))}
          </select>
        </label>
      ) : (
        <span class="dim" data-ov-staff-machine-one>
          On {label(can[0])}
        </span>
      )}
      <button type="button" class="primary" disabled={!on || busy || step.at === "grant"} data-ov-staff-go onClick={() => on && void s.start(on)}>
        {busy ? "Starting…" : go}
      </button>
    </div>
  );
}

/** None of the reader's machines can run it, and the ones left out and why. */
function StaffCannot({ s }: { s: StaffStarting }) {
  return (
    <>
      {s.can.length === 0 && (
        <p class="dim" data-ov-staff-none>
          None of your machines can run it now: start one that's online here, on the newest Arugula.
        </p>
      )}
      {s.cannot.length > 0 && (
        <p class="dim ov-staff-cannot" data-ov-staff-cannot>
          Not listed: {s.cannot.map((c) => `${shortHost(c.name)} (${c.why})`).join(", ")}.
        </p>
      )}
    </>
  );
}

/** The card asking for the machine's grant, and why a start failed. */
function StaffAfter({ s }: { s: StaffStarting }) {
  const { step } = s;
  return (
    <>
      {step.at === "grant" && (
        <div class="ov-staff-grant" role="group" aria-label="Allow it" data-ov-staff-grant={step.spot.name}>
          <p>
            <b>Let {shortHost(step.spot.name)}'s agents read and post in your DMs?</b>
          </p>
          <p class="dim">Your chief of staff talks with you in a room only you and your agents read, one of your DMs. You can take it back in Settings.</p>
          <div class="ov-staff-grant-acts">
            <button type="button" class="primary" data-ov-staff-allow onClick={() => void s.allow(step.spot, step.powers)}>
              Allow and start
            </button>
            <button type="button" class="secondary" data-ov-staff-not-now onClick={() => s.setStep({ at: "idle" })}>
              Not now
            </button>
          </div>
        </div>
      )}
      {step.at === "failed" && (
        <p class="ov-staff-failed" role="alert" data-ov-staff-failed>
          {step.why}
        </p>
      )}
    </>
  );
}

/** The tab's body with no staff room yet. */
export function StaffStart({ project, fleet, hostedIn }: { project: string; fleet: Fleet; hostedIn: (id: string | undefined) => string | undefined }) {
  const s = useStaffStart(project, fleet, hostedIn);
  return (
    <div class="ov-staff-start" data-ov-staff-start>
      <EmptyState
        title="Start your chief of staff"
        why="An agent that keeps up with this project for you: it reads the boards and threads, tells you what moved and what needs you, and answers here when you write to it. It runs on one of your machines and wakes when something happens."
        action={s.can.length > 0 && <StaffGo s={s} go="Start" />}
      >
        <StaffCannot s={s} />
      </EmptyState>
      <StaffAfter s={s} />
    </div>
  );
}

/** Over the staff room while its chief of staff is asleep, its machine
 * offline or gone (Arugula #1453): Run it on another machine, on one the
 * reader picks as Start does. start_agent there moves it: the one that led
 * is told to stand down (or stopped, a hosted one), so the room keeps one
 * lead, and the room's head follows it there. */
export function StaffMove({ project, fleet, hostedIn, asleepOn }: { project: string; fleet: Fleet; hostedIn: (id: string | undefined) => string | undefined; asleepOn: string }) {
  const s = useStaffStart(project, fleet, hostedIn, asleepOn);
  return (
    <div class="ov-staff-move" data-ov-staff-move={asleepOn}>
      <p class="dim">Nothing hears what you write here while it's asleep. Run it on another machine and it moves there; the one on {shortHost(asleepOn)} stands down.</p>
      {s.can.length > 0 && <StaffGo s={s} go="Run it on another machine" />}
      <StaffCannot s={s} />
      <StaffAfter s={s} />
    </div>
  );
}
