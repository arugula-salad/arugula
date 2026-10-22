// Switching hosts (M4a): a button in the desktop bar and a section in the
// phone's sheet. Each host has its own sessions and tabs; switching shows
// that host's layout, connected straight to it.

import { Fragment } from "preact";
import { useState } from "preact/hooks";
import { directory } from "../hosts";
import { blockRoutesLabel } from "../blocks/block-route";
import type { Fleet } from "../fleet";
import { useSubscribe } from "../../shared/ui/hooks";
import { openMenu, type MenuItem } from "../../shared/ui/menu";
import { openSwarm } from "../swarm/route";
import { controlMenuItems, controlState, subscribeControlState } from "./control-state";
import { HOSTED_LABEL, HOSTED_MEANS } from "../../shared/hosted";

function seen(name: string): string {
  const h = directory.find(name);
  if (!h) return "";
  // A sandbox's state comes from its provider; it may be asleep.
  if (h.transport === "provider") return `${h.status ?? "?"} · ${h.provider?.provider ?? "sandbox"}`;
  if (h.transport === "control" && h.status === "online") return "online";
  if (h.last_seen_ms === null) return "not seen yet";
  const s = Math.max(0, Math.round((Date.now() - h.last_seen_ms) / 1000));
  const ago = s < 60 ? `${s}s` : s < 3600 ? `${Math.round(s / 60)}m` : s < 86400 ? `${Math.round(s / 3600)}h` : `${Math.round(s / 86400)}d`;
  return `seen ${ago} ago`;
}

let fleet: Fleet | null = null;
/** M25: every host's summary connection, for what the menu says of each. */
export function setFleet(f: Fleet) {
  fleet = f;
}

/** The fleet, if this page has one (#78: every host's conversations). */
export function getFleet(): Fleet | null {
  return fleet;
}

function ago(ms: number): string {
  const s = Math.max(0, Math.round((Date.now() - ms) / 1000));
  return s < 60 ? `${s}s` : s < 3600 ? `${Math.round(s / 60)}m` : s < 86400 ? `${Math.round(s / 3600)}h` : `${Math.round(s / 86400)}d`;
}

/** How a machine is, in the same words wherever it's shown: the host
 * menu, Devices and machines, Teams, the phone's sheet. Control's word
 * comes first (a machine it says is offline is offline, though this page
 * keeps trying it), then this page's own connection to it. */
export function machineState(name: string): string {
  const d = directory.find(name);
  if (d?.transport === "provider") return `${d.status ?? "?"} · ${d.provider?.provider ?? "sandbox"}`;
  if (d?.transport === "control" && d.status === "offline") return d.last_seen_ms ? `offline, seen ${ago(d.last_seen_ms)} ago` : "offline";
  const h = fleet?.host(name);
  switch (h?.state) {
    case "connected":
      return "online";
    case "connecting":
      return "connecting";
    case "stale":
      return `reconnecting, seen ${h.lastSeen ? ago(h.lastSeen) : "?"} ago`;
    case "offline":
      return "offline";
    case "asleep":
      return "asleep";
    case "capped":
      return "online, not live here (too many machines)";
  }
  if (d?.transport === "control") return "online";
  return d ? seen(name) : "offline";
}

/** #778: where a machine is, in the same words wherever machines are
 * listed: *Just me*, or *In {team}* (a machine is in one team at a time).
 * Someone else's machine outside a team is theirs: you see what they share. */
export function machinePlace(team: string | null | undefined, owner?: string): string {
  if (team) return `In ${team}`;
  return owner ? `${owner}'s` : "Just me";
}

/** #778: what the place means: who sees its panes, who counts as its owner. */
export function placeMeans(team: string | null | undefined, owner?: string): string {
  if (team) return `everyone in ${team} sees its panes, except private ones, and ${team}'s owners count as its owners too.`;
  if (owner) return `you see what ${owner} shares with you.`;
  return "only your devices see its panes (and whoever you share a session with), and you're its only owner.";
}

/** A machine's place from the fleet, when control says whose it is. */
function placeIn(name: string): { place: string; means: string } | null {
  const h = fleet?.host(name);
  if (!directory.control || !h) return null;
  const team = h.team ? (h.teamName ?? "a team") : null;
  // M178: a hosted machine says so wherever its place is shown.
  if (h.hosted) return { place: team ? `In ${team} · ${HOSTED_LABEL}` : HOSTED_LABEL, means: HOSTED_MEANS };
  return { place: machinePlace(team, h.owner), means: placeMeans(team, h.owner) };
}

/** The host menu's line for a machine: its panes, when known, how it is,
 * and where it is (#778). */
export function fleetLabel(name: string): string {
  const n = fleet?.host(name)?.summary?.panes.length;
  const at = placeIn(name);
  return `${n === undefined ? "" : `${n} pane${n === 1 ? "" : "s"} · `}${machineState(name)}${at ? ` · ${at.place}` : ""}`;
}

/** More for the host menu (control mode: adding a machine). */
let extras: () => MenuItem[] = () => [];
export function setHostMenuExtras(f: () => MenuItem[]) {
  extras = f;
}

/** The machine settings the shown machine's menu adds (set by the app,
 * which has its client). */
let machineItems: () => MenuItem[] = () => [];
export function setMachineItems(f: () => MenuItem[]) {
  machineItems = f;
}

/** Whether the bar has the machine menu (else its settings go to the
 * account menu). */
export function hostMenuShown(): boolean {
  const dropped = controlState()?.state === "dropped";
  return directory.control || directory.names.length > 1 || directory.shown !== null || !!directory.joined || dropped;
}

/** Only worth showing once there is somewhere else to go. */
function useHosts(): boolean {
  useSubscribe((fn) => directory.subscribe(fn));
  useSubscribe((fn) => fleet?.subscribe(fn) ?? (() => {}));
  useSubscribe(subscribeControlState);
  // #325: a machine control dropped has the menu, to say so and join again.
  const dropped = controlState()?.state === "dropped";
  return directory.control || directory.names.length > 1 || directory.shown !== null || !!directory.joined || dropped;
}

/** M49: on a joined daemon's own page, the way to the account's other
 * machines is control's page, not a list here. */
function allMachines(): MenuItem[] {
  const url = directory.joined;
  if (directory.control || !url) return [];
  return [{ label: "All your machines…", run: () => void window.open(url, "_blank", "noopener") }];
}

/** M30: hosts grouped by whose they are (yours, each teammate's, each
 * team's); one group while there's only one person. */
function hostGroups(): { label: string; names: string[] }[] {
  const groups = new Map<string, { label: string; names: string[] }>();
  for (const name of directory.names) {
    const h = fleet?.host(name);
    const p = h ? fleet!.personOf(h) : { id: "me", name: "", kind: "me" as const };
    const label = p.kind === "me" ? "Yours" : p.kind === "team" ? `Team ${p.name}` : `${p.name}'s`;
    (groups.get(p.id) ?? groups.set(p.id, { label, names: [] }).get(p.id)!).names.push(name);
  }
  return [...groups.values()];
}

/** Desktop: the shown host, opening a menu of the others. */
export function HostButton({ path = true }: { path?: boolean } = {}) {
  if (!useHosts()) return null;
  const open = (e: MouseEvent) => {
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    const item = (name: string): MenuItem => ({
      label: `${name === directory.current ? "✓ " : "    "}${name}${name === directory.home ? " (home)" : ""}  · ${fleetLabel(name)}`,
      run: () => directory.select(name),
    });
    const groups = hostGroups();
    const items: MenuItem[] =
      groups.length > 1
        ? groups.flatMap((g) => [{ header: g.label } as MenuItem, ...g.names.map(item)])
        : directory.names.map(item);
    // M51: boxes reached over ssh, which only a terminal can open.
    if (directory.sshOnly.length) {
      items.push("separator", { header: "From a terminal (ssh)" } as MenuItem);
      for (const h of directory.sshOnly) {
        items.push({ label: `    ${h.name}  · arugula --host ${h.name} ls`, disabled: true, run: () => {} });
      }
    }
    items.push("separator", { label: "Swarm: every pane at once", run: openSwarm });
    // The shown machine's own settings: here, not in the account menu.
    const mine = machineItems();
    // #778: what the shown machine's place means, before someone joins or
    // shares from it.
    const at = placeIn(directory.current);
    const means: MenuItem[] = at ? [{ label: `${at.place}: ${at.means}`, disabled: true, run: () => {} }] : [];
    // M50: which way its blocks through control went, as the button says
    // for the machine's own connection.
    const blocks = blockRoutesLabel();
    if (blocks) means.push({ label: blocks, disabled: true, run: () => {} });
    if (mine.length || means.length) items.push("separator", { header: `This machine: ${directory.current}` } as MenuItem, ...means, ...mine);
    if (fleet?.notice) items.push("separator", { label: fleet.notice, disabled: true, run: () => {} });
    if (directory.stale) {
      const what = directory.control ? "Control unreachable: saved list" : "Home daemon unreachable: saved list";
      items.push("separator", { label: what, disabled: true, run: () => {} });
    }
    const all = allMachines();
    if (all.length) items.push("separator", ...all);
    // #325: whether and where this machine is joined to control.
    if (directory.shown === null && !directory.control) items.push(...controlMenuItems());
    items.push(...extras());
    openMenu({ clientX: r.left, clientY: r.bottom + 4, preventDefault: () => e.preventDefault() }, items);
  };
  return (
    <button class="host-button" title="Hosts" data-host={directory.current} onClick={open} onContextMenu={open}>
      {directory.current}
      {path && directory.path ? (
        <span class={`host-path ${directory.path}`} data-path={directory.path} title={directory.path === "relayed" ? "Through Arugula control's relay (end to end encrypted)" : "Straight to the machine"}>
          {directory.path}
        </span>
      ) : null}{" "}
      <span class="caret">▾</span>
    </button>
  );
}

/** While a host hasn't answered yet (it may be down): the way to another. */
export function HostPicker() {
  if (!useHosts()) return null;
  return (
    <div class="empty host-picker">
      <p>Connecting to {directory.current}…</p>
      {directory.names
        .filter((n) => n !== directory.current)
        .map((name) => (
          <button key={name} data-host={name} onClick={() => directory.select(name)}>
            Switch to {name}
          </button>
        ))}
    </div>
  );
}

/** Phone: the shown host's name in the header, when it isn't home. */
export function HostCrumb() {
  useSubscribe((fn) => directory.subscribe(fn));
  if (directory.shown === null && !directory.control) return null;
  return (
    <span class="host-crumb">
      {directory.current}
      {directory.path === "relayed" ? <span class="host-path relayed">relayed</span> : null}
    </span>
  );
}

/** Phone: a section of the sheet listing every host. */
export function HostSection({ close }: { close: () => void }) {
  if (!useHosts()) return null;
  const groups = hostGroups();
  return (
    <section class="sheet-hosts">
      <h2>Machines{directory.stale ? " (saved list)" : ""}</h2>
      {groups.map((g) => (
        <Fragment key={g.label}>
          {groups.length > 1 && <h3>{g.label}</h3>}
          <div class="sheet-chips">
            {g.names.map((name) => (
              <button
                key={name}
                class={name === directory.current ? "chip sheet-host current" : "chip sheet-host"}
                aria-pressed={name === directory.current}
                data-host={name}
                onClick={() => {
                  directory.select(name);
                  close();
                }}
              >
                <span class="host-name">{name}</span>
                <span class="host-seen" data-fleet={fleet?.host(name)?.state}>
                  {name === directory.home ? "home · " : ""}
                  {fleetLabel(name)}
                </span>
              </button>
            ))}
          </div>
        </Fragment>
      ))}
      <div class="sheet-chips">
        {[...allMachines(), ...extras()].flatMap((item) =>
          typeof item === "object" && "run" in item
            ? [
                <button
                  key={item.label}
                  class="chip"
                  onClick={() => {
                    item.run();
                    close();
                  }}
                >
                  {item.label}
                </button>,
              ]
            : [],
        )}
      </div>
    </section>
  );
}

const SEEN_KEY = "arugula.hosts.others";

function loadSeen(): Set<string> | null {
  try {
    const v = localStorage.getItem(SEEN_KEY);
    return v ? new Set(JSON.parse(v) as string[]) : null;
  } catch {
    return null;
  }
}

function saveSeen(seen: Set<string>) {
  try {
    localStorage.setItem(SEEN_KEY, JSON.stringify([...seen]));
  } catch {
    // Not remembered: said again on the next page.
  }
}

/** Machines someone else lets you reach, said once. Null until the first
 * list (what's there then isn't news). */
let others: Set<string> | null = loadSeen();

/** #551: someone else's machine that just reached your list (a share you
 * accepted, a team's machine): named here, not only in the host menu. */
export function NewMachineNote() {
  useSubscribe((fn) => directory.subscribe(fn));
  useSubscribe((fn) => fleet?.subscribe(fn) ?? (() => {}));
  const [, redraw] = useState(0);
  if (!fleet || directory.stale) return null;
  const theirs = directory.names.flatMap((name) => {
    const h = fleet!.host(name);
    // Someone else's: yours in a team isn't news to you.
    return h?.owner ? [{ name, owner: h.owner, team: h.teamName }] : [];
  });
  if (others === null) {
    others = new Set(theirs.map((m) => m.name));
    saveSeen(others);
    return null;
  }
  const seen = others;
  const done = (name: string) => {
    seen.add(name);
    saveSeen(seen);
    redraw((n) => n + 1);
  };
  // Shown already, by the host menu: found.
  if (theirs.some((x) => x.name === directory.current) && !seen.has(directory.current)) {
    seen.add(directory.current);
    saveSeen(seen);
  }
  const m = theirs.find((x) => !seen.has(x.name));
  if (!m) return null;
  return (
    <div class="machine-note" role="status" data-machine-note={m.name}>
      <p>
        You can reach {m.owner}'s machine <b>{m.name}</b>
        {m.team ? ` (team ${m.team})` : ""} now. Switch to it here, or later from the host menu.
      </p>
      <div class="machine-note-actions">
        <button
          class="primary"
          data-machine-note-show
          onClick={() => {
            done(m.name);
            directory.select(m.name);
          }}
        >
          Switch to {m.name}
        </button>
        <button onClick={() => done(m.name)}>Not now</button>
      </div>
    </div>
  );
}
