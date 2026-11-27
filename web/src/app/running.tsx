// Running (design lift §3.9, Lift 6a, Arugula #1483): one drawer over any
// place, every live process across your machines, from the fleet the page
// already reaches, plus the project's CI runs. Alerts sit at the top ("main
// has been red for 3 hours"). The arrow keys move through it; a pane's last
// lines peek under the list, and over a board, Pin to desk opens it beside
// the board (the place layout's `show`, #1496). Opened from the top bar's
// "N running"; it has no key of its own until #1534's list (`g r`, `p`).

import { useEffect, useState } from "preact/hooks";
import type { CiAlerts, RunView } from "../shared/proto";
import { agentName } from "../shared/agent-name";
import { redLine } from "../shared/boards/text";
import { Button } from "../shared/kit/button";
import { Row } from "../shared/kit/row";
import { Sheet } from "../shared/kit/sheet";
import { StateMark, type State } from "../shared/kit/state-mark";
import { Well } from "../shared/kit/well";
import { platformFetch } from "../shared/maintenance";
import { agentRunning } from "../shared/needs-you";
import { placeLayouts } from "../shared/place-layout";
import { usePlatformApi } from "../shared/platform-api";
import { cleanTitle } from "../shared/titles";
import { lastLines } from "../shared/transcript";
import { useSubscribe } from "../shared/ui/hooks";
import { ago } from "../shared/time";
import type { Fleet, FleetPane } from "../shell/fleet";
import { kindOf } from "../shell/swarm/model";
import { runs } from "../platform/runs";
import { boot } from "./machine-page";
import "./overview.css";
import type { AppSession } from "./session";

/** How many of a pane's last lines the peek shows, and how often it reads them (ms). */
const PEEK = 40;
const PEEK_EVERY_MS = 3000;

/** A pane's word in the drawer: its rung for an agent (lead, worker, agent), else what it is. */
function kindWord(p: FleetPane): string {
  const k = kindOf(p);
  if (k === "agent") return p.info.on_issue ? "worker" : "agent";
  return k === "server" ? "server" : k === "shell" ? "shell" : k;
}

/** Live: an agent that runs, or any other pane its machine still has. */
const live = (p: FleetPane) => !p.stale && p.info.reason?.kind !== "exited" && (kindOf(p) !== "agent" || agentRunning(p.info));

const stateOf = (p: FleetPane): State => (p.info.attention === "needs_input" ? "asks" : p.info.attention === "working" ? "working" : "idle");

type Picked = { kind: "pane"; key: string } | { kind: "run"; id: string } | null;

/** The drawer's opener and the drawer: `project` for its CI runs and alerts, `place` (`board:brd_…`) to pin to. */
export function RunningPlace({ project, place, session }: { project: string | null; place: string | null; session: AppSession | null }) {
  const [fleet, setFleet] = useState<Fleet | null>(null);
  const [open, setOpen] = useState(false);
  useEffect(() => {
    let on = true;
    void boot(session).then((b) => on && setFleet(b.machines.fleet));
    return () => void (on = false);
  }, []);
  useSubscribe((fn) => fleet?.subscribe(fn) ?? (() => {}));
  const panes = fleet ? fleet.panes.filter(live) : [];
  const working = panes.filter((p) => kindOf(p) === "agent").length;
  return (
    <>
      <button type="button" class="k-top-act" data-open-running={panes.length} title="Every live process across your machines" onClick={() => setOpen(true)}>
        <StateMark state={working ? "working" : "idle"} word={`${panes.length} running`} />
      </button>
      {open && fleet && <Drawer fleet={fleet} panes={panes} project={project} place={place} close={() => setOpen(false)} />}
    </>
  );
}

function Drawer({ fleet, panes, project, place, close }: { fleet: Fleet; panes: FleetPane[]; project: string | null; place: string | null; close: () => void }) {
  const api = usePlatformApi();
  const [alerts, setAlerts] = useState<CiAlerts["open"]>([]);
  const [going, setGoing] = useState<RunView[]>([]);
  const [picked, setPicked] = useState<Picked>(null);
  useEffect(() => {
    if (!project) return;
    let on = true;
    void platformFetch(`/api/projects/${encodeURIComponent(project)}/ci-alerts`, { credentials: "same-origin" })
      .then((r) => (r.ok ? (r.json() as Promise<CiAlerts>) : null))
      .then((a) => on && setAlerts(a?.on ? a.open : []))
      .catch(() => {});
    runs.list(project, { state: "running" }, 20).then(
      (l) => on && setGoing(l.runs),
      () => {},
    );
    return () => void (on = false);
  }, [project]);
  const hosts = [...new Set(panes.map((p) => p.host))];
  const order: Picked[] = [...panes.map((p) => ({ kind: "pane" as const, key: p.key })), ...going.map((r) => ({ kind: "run" as const, id: r.id }))];
  const at = picked ? order.findIndex((o) => (o?.kind === "pane" && picked.kind === "pane" ? o.key === picked.key : o?.kind === "run" && picked.kind === "run" && o.id === picked.id)) : -1;
  // Up and Down move through the list, as a list's own keys (not the page's table).
  const keys = (e: KeyboardEvent) => {
    const by = e.key === "ArrowDown" ? 1 : e.key === "ArrowUp" ? -1 : 0;
    if (!by || !order.length) return;
    e.preventDefault();
    const next = order[Math.max(0, Math.min(order.length - 1, at < 0 ? 0 : at + by))]!;
    setPicked(next);
    requestAnimationFrame(() => document.querySelector<HTMLElement>(`[data-running-row="${next?.kind === "pane" ? next.key : next?.id}"] .k-row-line`)?.focus());
  };
  const pane = picked?.kind === "pane" ? panes.find((p) => p.key === picked.key) : undefined;
  const run = picked?.kind === "run" ? going.find((r) => r.id === picked.id) : undefined;
  const pin = async () => {
    if (!place || !pane) return;
    const machine = fleet.host(pane.host)?.id ?? pane.host;
    const title = cleanTitle(pane.info.title ?? "") || agentName({ role: "agent", session: pane.session?.name, machine: pane.host });
    try {
      await placeLayouts.change(place, { op: "show", pane: { kind: "terminal", ref: `%${pane.id}`, title, machine, state: null }, intent: "beside", tab: null, in_place_of: null });
      api.toast(`Pinned ${title} beside the board`);
    } catch (e) {
      api.toast(`Couldn't pin it: ${(e as Error).message}`);
    }
  };
  return (
    <Sheet side title="Running" onClose={close}>
      <div data-running onKeyDown={keys}>
        <p class="m-meta" data-running-count>
          {panes.length} live · {hosts.length} {hosts.length === 1 ? "machine" : "machines"}
        </p>
        {alerts.length > 0 && (
          <section aria-label="Alerts" data-running-alerts>
            {alerts.map((a) => (
              <div key={a.issue} role="alert" data-running-alert={a.branch}>
                <Row mark="failing" label="alert" title={redLine(a)} />
              </div>
            ))}
          </section>
        )}
        {hosts.map((h) => {
          const list = panes.filter((p) => p.host === h);
          return (
            <section key={h} aria-label={h} data-running-host={h}>
              <p class="m-meta">
                {h} · {list.length}
              </p>
              {list.map((p) => (
                <Row
                  key={p.key}
                  mark={stateOf(p)}
                  title={cleanTitle(p.info.title ?? "") || p.session?.name || `pane %${p.id}`}
                  meta={[kindWord(p), p.info.activity?.last_ms ? ago(p.info.activity.last_ms) : ""].filter(Boolean)}
                  kind={pane?.key === p.key ? "selected" : "plain"}
                  onClick={() => setPicked({ kind: "pane", key: p.key })}
                  data-running-row={p.key}
                />
              ))}
            </section>
          );
        })}
        {going.length > 0 && (
          <section aria-label="CI runs" data-running-runs>
            <p class="m-meta">CI runs · {going.length}</p>
            {going.map((r) => (
              <Row key={r.id} mark="working" title={r.id} meta={["ci run", r.started_at ? ago(r.started_at) : ""].filter(Boolean)} kind={run?.id === r.id ? "selected" : "plain"} onClick={() => setPicked({ kind: "run", id: r.id })} data-running-row={r.id} />
            ))}
          </section>
        )}
        {panes.length === 0 && going.length === 0 && <p class="m-meta">Nothing is running.</p>}
        {pane && (
          <section class="running-peek" aria-label={`${pane.host} %${pane.id}`} data-running-peek={pane.key}>
            <Row mark={stateOf(pane)} title={kindWord(pane)} meta={[pane.host]} />
            {place && (
              <Button kind="plain" onClick={() => void pin()} data-running-pin>
                Pin to desk
              </Button>
            )}
            <Peek fleet={fleet} pane={pane} />
          </section>
        )}
        {run && project && (
          <section class="running-peek" data-running-peek={run.id}>
            <a class="k-button is-plain" href={`/p/${project}/runs/${run.id}`} data-running-run-open>
              Open the run
            </a>
          </section>
        )}
      </div>
    </Sheet>
  );
}

/** A pane's last lines, read again while it's shown. */
function Peek({ fleet, pane }: { fleet: Fleet; pane: FleetPane }) {
  const [lines, setLines] = useState<string[] | null>(null);
  useEffect(() => {
    setLines(null);
    let on = true;
    const read = async () => {
      try {
        const res = await fleet.request(pane.host, "GET", `/api/panes/${pane.id}/capture?format=text`);
        const text = res.ok && res.text ? await res.text() : "";
        const all = text.split("\n").map((l) => l.trimEnd());
        while (all.length && !all[all.length - 1]) all.pop();
        if (on) setLines(lastLines(all, false, PEEK));
      } catch {
        // Its machine isn't reachable now: what it had stays.
      }
    };
    void read();
    const t = setInterval(() => void read(), PEEK_EVERY_MS);
    return () => {
      on = false;
      clearInterval(t);
    };
  }, [pane.key]);
  return <Well label="Last lines">{lines === null ? "Reading…" : lines.join("\n") || "Nothing yet."}</Well>;
}
