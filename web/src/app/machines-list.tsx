// The Machines page (Arugula #1346): every machine with sessions in the
// project, a section each, its sessions as cards and each card's panes as
// rows. It took the place of Build's list of panes (#1083), the grid of
// machines (#990) and, in time, a machine's "Sessions on <host>" overview.
// This computer leads, then online before offline, yours before
// teammates'; machines with nothing of the project fold into one line at
// the bottom. The sessions you pinned come first (#1261), and the pane you
// had open last stays lit. Only the project's sessions (#1164). The
// address can name a machine (`show`) and a session's card on it (`card`):
// the page scrolls there and marks it (Arugula #1349), as the shell's
// palette, its mark and its session crumb open it.

import "./machines-list.css";
import { useLayoutEffect, useRef, useState } from "preact/hooks";
import { parseMachines, type MachineAt } from "../shared/machine-paths";
import { needsYou, NEEDS_YOU } from "../shared/needs-you";
import { EmptyState } from "../shared/ui/block-states";
import { Card } from "../shared/ui/card";
import { usePhone, useSubscribe } from "../shared/ui/hooks";
import { Caret, IconButton, PinIcon } from "../shared/ui/icons";
import { PINNED, unpin } from "../shared/session-pins";
import { sessionPins } from "../shared/session-pins-store";
import { machineOfHost } from "../shell/pins";
import { menuBelow, openMenu } from "../shared/ui/menu";
import { PageHeader } from "../shared/ui/page-header";
import { Chip, StateChip } from "../shared/ui/state-chip";
import { directory } from "../shell/hosts";
import type { Fleet, FleetPane } from "../shell/fleet";
import { KIND_LABEL, kindOf, shortHost, whatItDoes } from "../shell/swarm/model";
import { lastTabOf } from "../shell/ui/shell/session-sidebar";
import { names } from "../platform/links";
import { session as platform } from "../platform/session";
import { paneOf } from "../platform/places-kept";
import { follow, go, pathOf } from "../platform/router";
import { machineSections, pinnedCards, statsLine, type MachineSection, type SessionCard } from "./machine-list";
import { inShown, onShownProject } from "../shell/project-view";
import { AddMachine } from "./add-machine";
import type { AppSession } from "./session";

type Last = { machine?: string; pane: number } | null;

/** The pane you last had open in `project` (its machine's id, if the path
 * names one), from the path kept for it. */
function lastPane(project: string): Last {
  const kept = paneOf(project);
  if (!kept) return null;
  const url = new URL(kept, location.origin);
  const rest = url.pathname.split("/").slice(4);
  const at = parseMachines(rest, url.searchParams);
  if (!at || at.at === "swarm" || at.at === "panes" || at.pane === undefined) return null;
  return { machine: at.at === "machine" ? at.machine : undefined, pane: at.pane };
}

const home = () => directory.home ?? directory.names[0];

/** Whether `p`, on `m`, is that pane. A machine without an id is the page's own daemon's (the home one). */
const isLast = (last: Last, m: MachineSection, p: FleetPane) => !!last && last.pane === p.id && (m.id ? last.machine === m.id : !last.machine || m.name === home());

/** A machine's place: by its id, else (a daemon's own page) by its name;
 * a pane on the home machine as a notification's, naming none. */
function machineAt(m: Pick<MachineSection, "id" | "name">, pane?: number): MachineAt {
  if (m.id) return { at: "machine", machine: m.id, ...(pane !== undefined ? { pane } : {}) };
  return pane !== undefined ? { at: "machines", pane, ...(m.name !== home() ? { name: m.name } : {}) } : { at: "machines", name: m.name };
}

/** The pane a session opens at: the one you had open last, if it's this
 * session's; else its last tab's in this page; else its first. */
function sessionPane(m: MachineSection, s: SessionCard, last: Last): number {
  const panes = s.tabs.flatMap((t) => t.panes);
  const mine = panes.find((p) => isLast(last, m, p));
  if (mine) return mine.id;
  let base = "";
  try {
    base = directory.base(m.name);
  } catch {
    // gone from the list: no tab kept for it
  }
  const tab = s.tabs.find((t) => t.id === lastTabOf(base, s.id));
  return (tab ?? s.tabs[0]).panes[0].id;
}

const isAgent = (p: FleetPane) => kindOf(p) === "agent";

/** A card shows this many tabs, and "+N more" for the rest, so one busy
 * session doesn't push the next machine off a laptop's first screen. */
const TABS = 3;

type To = (at: MachineAt) => string;

export function MachinesList({
  project,
  fleet,
  agents,
  show,
  card,
  add,
  session,
}: {
  project: string;
  fleet: Fleet;
  agents: boolean;
  show?: string;
  card?: number;
  /** Add a machine open (`?add`). */
  add: boolean;
  session: AppSession | null;
}) {
  useSubscribe((fn) => directory.subscribe(fn));
  useSubscribe((fn) => fleet.subscribe(fn));
  useSubscribe((fn) => session?.subscribe(fn) ?? (() => {}));
  useSubscribe((fn) => platform.subscribe(fn));
  useSubscribe(onShownProject);
  useSubscribe((fn) => sessionPins.subscribe(fn));
  const phone = usePhone();
  const to: To = (at) => pathOf({ page: "machines", project, at }, names);
  const here = (a: boolean, adding = false) => to({ at: "machines", ...(a ? { agents: true } : {}), ...(adding ? { add: true } : {}) });
  const p = platform.project(project);
  const name = platform.project(project)?.name ?? "this project";
  const hosted = (id: string | undefined) => (id ? session?.daemons.find((d) => d.id === id)?.hosted : undefined);
  const last = lastPane(project);
  // This computer is the page's own daemon's; in a browser, the machine you used last leads yours.
  const { machines, quiet } = machineSections(fleet.list, fleet.panes, {
    shows: (p, yours) => inShown(p, yours),
    here: directory.home,
    recent: last?.machine ?? null,
    only: agents ? isAgent : undefined,
    // With nothing of the project: your machines, and the project's cloud workspaces.
    keep: (h) => (h.hosted ? hosted(h.id)?.project === project : !h.owner),
  });
  const idOf = (m: MachineSection) => m.id ?? machineOfHost(m.name);
  if (machines.some(idOf)) sessionPins.want();
  const { pinned, machines: below } = pinnedCards(machines, sessionPins.on ? sessionPins.pins : [], idOf);
  const cards = machines.flatMap((m) => m.sessions);
  // The machine the address names, by its id or its name, and its session's card.
  const named = show ? [...machines, ...quiet].find((m) => m.id === show || m.name === show || idOf(m) === show) : undefined;
  const at = named && card !== undefined && named.sessions.some((s) => s.id === card) ? card : undefined;
  const picked = (m: MachineSection) => !!named && m.name === named.name;
  const root = useRef<HTMLElement>(null);
  const scrolled = useRef<string | null>(null);
  // Scrolled to it once it's drawn, once for each address.
  useLayoutEffect(() => {
    const key = named ? `${named.name}/${at ?? ""}` : null;
    if (!key || scrolled.current === key) return;
    const el = root.current?.querySelector<HTMLElement>("[data-machines-picked]");
    if (!el) return;
    scrolled.current = key;
    el.scrollIntoView({ block: at === undefined ? "start" : "center" });
    el.querySelector<HTMLElement>("[data-machines-session-link], [data-machines-machine-link]")?.focus({ preventScroll: true });
  });
  return (
    <section class="platform-section sw-page machines-page" data-machines-list ref={root}>
      <PageHeader
        title="Machines"
        stats={
          (machines.length > 0 || quiet.length > 0) && (
            <span class="dim" data-machines-stats>
              {statsLine(machines.length + quiet.length, cards)}
            </span>
          )
        }
        actions={
          <>
            <button class="secondary" aria-pressed={agents} data-machines-agents onClick={() => go(here(!agents), true)}>
              Agents only
            </button>
            {p && (
              <button class="secondary" data-machines-add onClick={() => go(here(agents, true))}>
                Add a machine
              </button>
            )}
            <NewSession fleet={fleet} to={to} />
          </>
        }
      />
      {add && p && <AddMachine p={p} onClose={() => go(here(agents), true)} />}
      {directory.list === null ? (
        <p class="dim">Loading…</p>
      ) : !fleet.list.length ? (
        <EmptyState where="page" title="No machines yet." why="A machine shows here once it has joined your team. Its sessions and who may steer them come with it." data-machines-empty />
      ) : !machines.length && agents ? (
        <EmptyState
          where="page"
          title="No agents on your machines."
          why="A session with an agent in it shows here."
          action={
            <a class="secondary" href={here(false)} data-machines-all onClick={(e) => follow(e, here(false))}>
              Every session
            </a>
          }
          data-machines-no-agents
        />
      ) : (
        <>
          {!machines.length && <EmptyState where="page" title={`No ${name} sessions yet.`} why="Start one with New session." data-machines-empty data-project-empty />}
          {pinned.length > 0 && (
            <section class="machines-machine" data-machines-pinned aria-labelledby="machines-pinned-head">
              <header class="machines-head">
                <span class="machines-pinned-title" role="heading" aria-level={2} id="machines-pinned-head">
                  <PinIcon size="sm" /> {PINNED}
                </span>
              </header>
              <ul class="machines-sessions">
                {pinned.map(({ pin, machine: m, session: s }) => (
                  <Session
                    key={`${pin.machine}/${pin.session}`}
                    m={m}
                    s={s}
                    to={to}
                    last={last}
                    phone={phone}
                    picked={picked(m) && s.id === at}
                    pinned={() => void sessionPins.edit((pins) => unpin(pins, pin.machine, pin.session))}
                    data-machines-pinned-session={`${pin.machine}/${pin.session}`}
                  />
                ))}
              </ul>
            </section>
          )}
          {below.map((m) => (
            <Machine key={m.name} m={m} count={machines.find((x) => x.name === m.name)!.sessions.length} to={to} last={last} phone={phone} picked={picked(m)} card={picked(m) ? at : undefined} />
          ))}
          {quiet.length > 0 && <Quiet quiet={quiet} to={to} name={name} picked={quiet.find(picked)?.name} />}
        </>
      )}
    </section>
  );
}

/** A machine's head: its name (its page), whether it's on, Cloud, this
 * computer, whose it is; `count`, its sessions in the project. */
function Head({ m, to, count }: { m: MachineSection; to: To; count?: number }) {
  const page = to(machineAt(m));
  return (
    <>
      <a class="machines-name" href={page} title={`${m.name}: its page`} data-machines-machine-link onClick={(e) => follow(e, page)}>
        {shortHost(m.name)}
      </a>
      <StateChip tone={m.online ? "ok" : "muted"} data-machines-state>
        {m.online ? "online" : "offline"}
      </StateChip>
      {m.cloud && <Chip kind="tag">Cloud</Chip>}
      {m.here && (
        <Chip kind="tag" data-machines-here>
          this computer
        </Chip>
      )}
      <span class="dim">{m.owner ? `${m.owner}'s` : "Yours"}</span>
      {count !== undefined && (
        <span class="dim machines-count">
          {count} {count === 1 ? "session" : "sessions"}
        </span>
      )}
    </>
  );
}

/** `picked`: the machine the address names, marked unless it names one of
 * its sessions' cards (`card`), which is. */
function Machine({ m, count, to, last, phone, picked, card }: { m: MachineSection; count: number; to: To; last: Last; phone: boolean; picked: boolean; card?: number }) {
  return (
    <section class="machines-machine" data-machines-group={m.id ?? m.name} data-machines-picked={(picked && card === undefined) || undefined} aria-label={shortHost(m.name)}>
      <header class="machines-head">
        <Head m={m} to={to} count={count} />
      </header>
      <ul class="machines-sessions">
        {m.sessions.map((s) => (
          <Session key={s.id} m={m} s={s} to={to} last={last} phone={phone} picked={s.id === card} />
        ))}
      </ul>
    </section>
  );
}

/** A session's card: its state, its name (it opens at its last tab), its
 * counts, and its panes, under their tab's name when it has more than one.
 * Its first few tabs, and any with a pane that needs you or the one you had
 * open last; "+N more" opens the rest in place. A pinned one says so, and
 * unpins (`pinned`), and names its machine. */
function Session({ m, s, to, last, phone, picked, pinned, ...data }: { m: MachineSection; s: SessionCard; to: To; last: Last; phone: boolean; picked?: boolean; pinned?: () => void; [k: `data-${string}`]: unknown }) {
  const [all, setAll] = useState(false);
  const open = to(machineAt(m, sessionPane(m, s, last)));
  const tabs = s.tabs.length;
  const few = s.tabs.filter((t, i) => i < TABS || t.panes.some((p) => needsYou(p.info) || isLast(last, m, p)));
  const more = tabs - few.length;
  return (
    <li class="machines-session-item">
      <Card as="article" class="machines-session" selected={picked} data-machines-session={s.id} data-machines-picked={picked || undefined} data-attention={s.state} {...data}>
        <header class="machines-session-head">
          <span class={`ws-dot ${s.state}`} aria-hidden="true" />
          <a class="machines-session-name" href={open} title={`Open ${s.name} at its last tab`} data-machines-session-link onClick={(e) => follow(e, open)}>
            {s.name}
          </a>
          {pinned && (
            <IconButton label={`Unpin ${s.name}`} title="Unpin session" class="machines-unpin" data-machines-unpin onClick={pinned}>
              <PinIcon size="sm" />
            </IconButton>
          )}
        </header>
        <p class="machines-session-meta dim">
          {pinned && `on ${shortHost(m.name)} · `}
          {tabs} {tabs === 1 ? "tab" : "tabs"} · {s.panes} {s.panes === 1 ? "pane" : "panes"}
        </p>
        {(all ? s.tabs : few).map((t) => (
          <div key={t.id} data-machines-tab={t.id}>
            {tabs > 1 && <h4 class="machines-tab dim">{t.name ?? `Tab ${s.tabs.indexOf(t) + 1}`}</h4>}
            <ul class="panes-rows">
              {t.panes.map((p) => (
                <PaneLine key={p.key} p={p} to={to(machineAt(m, p.id))} last={isLast(last, m, p)} phone={phone} />
              ))}
            </ul>
          </div>
        ))}
        {more > 0 && (
          <button type="button" class="link small machines-more" aria-expanded={all} data-machines-more onClick={() => setAll(!all)}>
            {all ? "Show fewer" : `+${more} more ${more === 1 ? "tab" : "tabs"}`}
          </button>
        )}
      </Card>
    </li>
  );
}

function PaneLine({ p, to, last, phone }: { p: FleetPane; to: string; last: boolean; phone: boolean }) {
  const asks = needsYou(p.info);
  const what = whatItDoes(p);
  // A pane that says nothing of itself reads as its kind: once.
  const kind = KIND_LABEL[kindOf(p)];
  const meta = phone ? "" : [kind !== what && kind, p.info.branch].filter(Boolean).join(" · ");
  return (
    <li>
      <a class={`panes-row${p.stale ? " stale" : ""}${last ? " last" : ""}`} href={to} title={last ? "The pane you had open last" : undefined} data-panes-pane={p.id} data-panes-host={p.host} data-panes-last={last || undefined} onClick={(e) => follow(e, to)}>
        <span class="panes-row-what">{what}</span>
        {last && <span class="panes-row-where dim">where you were</span>}
        {asks && <StateChip tone="warn">{NEEDS_YOU}</StateChip>}
        {meta && <span class="panes-row-meta dim">{meta}</span>}
      </a>
    </li>
  );
}

/** The machines with no session in the project: one line, which opens to a
 * list, open with the one the address names (`picked`). */
function Quiet({ quiet, to, name, picked }: { quiet: MachineSection[]; to: To; name: string; picked?: string }) {
  const n = quiet.length;
  return (
    <details class="machines-quiet" open={!!picked || undefined} data-machines-quiet>
      <summary class="dim">
        {n} {n === 1 ? "machine" : "machines"} with no {name} sessions
      </summary>
      <ul class="machines-quiet-list">
        {quiet.map((m) => (
          <li key={m.name} class="machines-head" data-machines-group={m.id ?? m.name} data-machines-picked={m.name === picked || undefined}>
            <Head m={m} to={to} />
          </li>
        ))}
      </ul>
    </details>
  );
}

/** New session: on this computer, else your one machine that's on, else
 * the one you pick; its first pane opens once it's there. */
function NewSession({ fleet, to }: { fleet: Fleet; to: To }) {
  const yours = fleet.list.filter((h) => !h.owner && !h.hosted && (h.status ? h.status !== "offline" : h.state === "connected") && fleet.clientOf(h.name));
  if (!yours.length) return null;
  const start = (name: string) => {
    const c = fleet.clientOf(name);
    if (!c) return;
    const before = new Set(fleet.host(name)?.summary?.sessions.map((s) => s.id) ?? []);
    c.intent({ op: "new_session", name: null, from_pane: null });
    const off = fleet.subscribe(() => {
      const h = fleet.host(name);
      const st = h?.summary;
      const s = st?.sessions.find((x) => !before.has(x.id));
      const pane = s && st?.tabs.find((t) => t.id === s.tabs[0])?.layout.panes[0]?.[0];
      if (pane === undefined) return;
      off();
      clearTimeout(timer);
      go(to(machineAt({ id: h!.id, name }, pane)));
    });
    const timer = setTimeout(off, 15_000);
  };
  const here = yours.find((h) => h.name === directory.home);
  const pick = (e: MouseEvent) => {
    if (here || yours.length === 1) return start((here ?? yours[0]).name);
    openMenu(
      menuBelow(e.currentTarget as HTMLElement),
      yours.map((h) => ({ label: shortHost(h.name), run: () => start(h.name) })),
      { from: e.currentTarget as HTMLElement, label: "New session on" },
    );
  };
  return (
    <button class="secondary" data-machines-new-session onClick={pick}>
      New session{!here && yours.length > 1 && <Caret />}
    </button>
  );
}
