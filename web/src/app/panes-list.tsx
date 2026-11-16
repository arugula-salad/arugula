// Build's home (Arugula #1083): every machine's panes in one list, a
// section per machine, read from the fleet's summaries as Swarm's are.
// The machine is a filter over it, not where it starts, and Agents only
// another (#1082: where the Sessions page's agents went); a row opens the
// pane on its machine's page. Opinionated: no options but the filters.

import "./panes-list.css";
import { parseMachines, type MachineAt } from "../shared/machine-paths";
import { needsYou, NEEDS_YOU } from "../shared/needs-you";
import { EmptyState } from "../shared/ui/block-states";
import { usePhone, useSubscribe } from "../shared/ui/hooks";
import { Caret } from "../shared/ui/icons";
import { menuBelow, openMenu } from "../shared/ui/menu";
import { PageHeader } from "../shared/ui/page-header";
import { Chip, StateChip } from "../shared/ui/state-chip";
import { directory } from "../shell/hosts";
import type { Fleet, FleetPane } from "../shell/fleet";
import { KIND_LABEL, kindOf, shortHost, whatItDoes } from "../shell/swarm/model";
import { names } from "../platform/links";
import { paneOf } from "../platform/places-kept";
import { follow, go, pathOf } from "../platform/router";
import { paneGroups, type PaneGroup } from "./machine-list";

/** The pane you last had open in `project` (its machine's id, if the path
 * names one), from the path kept for it. */
function lastPane(project: string): { machine?: string; pane: number } | null {
  const kept = paneOf(project);
  if (!kept) return null;
  const url = new URL(kept, location.origin);
  const rest = url.pathname.split("/").slice(4);
  const at = parseMachines(rest, url.searchParams);
  if (!at || at.at === "swarm" || at.at === "panes" || at.pane === undefined) return null;
  return { machine: at.at === "machine" ? at.machine : undefined, pane: at.pane };
}

/** Whether `p`, on `g`, is that pane. A machine without an id is the page's own daemon's (the home one). */
const isLast = (last: { machine?: string; pane: number } | null, g: PaneGroup, p: FleetPane) =>
  !!last && last.pane === p.id && (g.id ? last.machine === g.id : !last.machine || g.name === home());

const home = () => directory.home ?? directory.names[0];

/** A machine's place: by its id, else (a daemon's own page) by its name;
 * a pane on the home machine as a notification's, naming none. */
function machineAt(g: Pick<PaneGroup, "id" | "name">, pane?: number): MachineAt {
  if (g.id) return { at: "machine", machine: g.id, ...(pane !== undefined ? { pane } : {}) };
  return pane !== undefined ? { at: "machines", pane, ...(g.name !== home() ? { name: g.name } : {}) } : { at: "machines", name: g.name };
}

const isAgent = (p: FleetPane) => kindOf(p) === "agent";

/** `agents`: only the panes an agent is in. */
export function PanesList({ project, fleet, machine, agents }: { project: string; fleet: Fleet; machine?: string; agents: boolean }) {
  useSubscribe((fn) => directory.subscribe(fn));
  useSubscribe((fn) => fleet.subscribe(fn));
  const phone = usePhone();
  const to = (at: MachineAt) => pathOf({ page: "machines", project, at }, names);
  // This list, filtered so: the machine's and Agents only's links keep each other.
  const here = (f: { machine?: string; agents: boolean }) => to({ at: "panes", ...(f.machine ? { machine: f.machine } : {}), ...(f.agents ? { agents: true } : {}) });
  const all = paneGroups(fleet.list, fleet.panes);
  const mine = machine ? all.filter((g) => g.id === machine || (!g.id && g.name === machine)) : all;
  const picked = machine ? mine[0] : undefined;
  // Agents only drops the machines with none, unless you picked that one.
  const groups = agents ? mine.map((g) => ({ ...g, panes: g.panes.filter(isAgent) })).filter((g) => machine || g.panes.length) : mine;
  const panes = groups.reduce((n, g) => n + g.panes.length, 0);
  const asking = groups.reduce((n, g) => n + g.panes.filter((p) => needsYou(p.info)).length, 0);
  const pick = (e: MouseEvent) =>
    openMenu(
      menuBelow(e.currentTarget as HTMLElement),
      [
        { label: "Every machine", checked: !machine, run: () => go(here({ agents }), true) },
        ...all.map((g) => ({
          label: shortHost(g.name),
          detail: `${g.panes.length} ${g.panes.length === 1 ? "pane" : "panes"}${g.online ? "" : " · offline"}`,
          faded: !g.online,
          checked: !!picked && picked.name === g.name,
          run: () => go(here({ machine: g.id ?? g.name, agents }), true),
        })),
      ],
      { from: e.currentTarget as HTMLElement, label: "Machine" },
    );
  return (
    <section class="platform-section sw-page panes-page" data-panes-list>
      <PageHeader
        title="Panes"
        stats={
          panes > 0 && (
            <span class="dim" data-panes-stats>
              {panes} {agents ? (panes === 1 ? "agent" : "agents") : panes === 1 ? "pane" : "panes"}
              {asking > 0 && ` · ${asking} ${asking === 1 ? "needs" : "need"} you`}
            </span>
          )
        }
      />
      <div class="issues-tools panes-tools">
        <button class="secondary" aria-pressed={!!machine} data-panes-machine={machine ?? ""} onClick={pick}>
          {picked ? `On ${shortHost(picked.name)}` : machine ? "On a machine" : "Every machine"} <Caret />
        </button>
        <button class="secondary" aria-pressed={agents} data-panes-agents onClick={() => go(here({ machine, agents: !agents }), true)}>
          Agents only
        </button>
      </div>
      {directory.list === null ? (
        <p class="dim">Loading…</p>
      ) : !all.length ? (
        <EmptyState where="page" title="No machines yet." why="A machine's panes show here once it has joined your team." data-panes-empty />
      ) : !mine.length ? (
        <EmptyState
          where="page"
          title="That machine isn't one you can reach."
          action={
            <a class="secondary" href={here({ agents })} data-panes-every onClick={(e) => follow(e, here({ agents }))}>
              Every machine's panes
            </a>
          }
          data-panes-unknown
        />
      ) : !groups.length ? (
        <EmptyState
          where="page"
          title="No agents on your machines."
          why="A pane with an agent in it shows here."
          action={
            <a class="secondary" href={here({ agents: false })} data-panes-all onClick={(e) => follow(e, here({ agents: false }))}>
              Every pane
            </a>
          }
          data-panes-no-agents
        />
      ) : (
        groups.map((g) => <Group key={g.name} g={g} to={to} last={lastPane(project)} phone={phone} agents={agents} />)
      )}
    </section>
  );
}

/** `last`: the pane you last had open in the project. */
function Group({ g, to, last, phone, agents }: { g: PaneGroup; to: (at: MachineAt) => string; last: { machine?: string; pane: number } | null; phone: boolean; agents: boolean }) {
  const page = to(machineAt(g));
  return (
    <section class="panes-machine" data-panes-group={g.id ?? g.name} aria-label={shortHost(g.name)}>
      <header class="panes-machine-head">
        <a class="panes-machine-name" href={page} title={`${g.name}: its page`} data-panes-machine-link onClick={(e) => follow(e, page)}>
          {shortHost(g.name)}
        </a>
        <StateChip tone={g.online ? "ok" : "muted"}>{g.online ? "online" : "offline"}</StateChip>
        {g.cloud && <Chip kind="tag">Cloud</Chip>}
        <span class="dim">{g.owner ? `${g.owner}'s` : "Yours"}</span>
      </header>
      {g.panes.length ? (
        <ul class="panes-rows">
          {g.panes.map((p) => (
            <Row key={p.key} p={p} to={to(machineAt(g, p.id))} last={isLast(last, g, p)} phone={phone} />
          ))}
        </ul>
      ) : (
        <p class="dim panes-none">{agents ? "No agents on it." : g.online ? "No panes open." : "No panes heard from it yet."}</p>
      )}
    </section>
  );
}

function Row({ p, to, last, phone }: { p: FleetPane; to: string; last: boolean; phone: boolean }) {
  const asks = needsYou(p.info);
  const what = whatItDoes(p);
  // A pane that says nothing of itself reads as its kind: once.
  const kind = KIND_LABEL[kindOf(p)];
  return (
    <li>
      <a class={`panes-row${p.stale ? " stale" : ""}${last ? " last" : ""}`} href={to} title={last ? "The pane you had open last" : undefined} data-panes-pane={p.id} data-panes-host={p.host} data-panes-last={last || undefined} onClick={(e) => follow(e, to)}>
        <span class="panes-row-what">{what}</span>
        {asks && <StateChip tone="accent">{NEEDS_YOU}</StateChip>}
        <span class="panes-row-meta dim">
          {[kind !== what && kind, !phone && p.session?.name, !phone && p.info.branch].filter(Boolean).join(" · ")}
        </span>
      </a>
    </li>
  );
}
