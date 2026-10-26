// Switching hosts (M4a): a button in the desktop bar and a section in the
// phone's sheet. Each host has its own sessions and tabs; switching shows
// that host's layout, connected straight to it.

import { Fragment } from "preact";
import { useEffect, useRef, useState } from "preact/hooks";
import { directory } from "../hosts";
import { blockRoutesLabel } from "../blocks/block-route";
import type { Fleet } from "../fleet";
import { useSubscribe } from "../../shared/ui/hooks";
import { openMenu, useMenuOpenFrom, type MenuItem } from "../../shared/ui/menu";
import { SWARM_ABOUT, openSwarm } from "../swarm/route";
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

/** The machine picker's row (UI audit #175): the name, then, dim, what
 * its group's header doesn't say (its panes, how it is, whose it is in a
 * team, hosted). Its place and what that means are its tooltip. The shown
 * one has the ● the account menu uses; an offline one is dim. */
function pickerItem(name: string): MenuItem {
  const h = fleet?.host(name);
  const n = h?.summary?.panes.length;
  const state = machineState(name);
  const at = placeIn(name);
  const detail = [n === undefined ? "" : `${n} pane${n === 1 ? "" : "s"}`, state, h?.team && h.owner ? `${h.owner}'s` : "", h?.hosted ? "hosted" : ""];
  return {
    label: `${name}${name === directory.home ? " (home)" : ""}`,
    detail: detail.filter(Boolean).join(" · "),
    current: name === directory.current,
    faded: dotOf(state) === "off",
    hint: at ? `${at.place}: ${at.means}` : undefined,
    run: () => directory.select(name),
  };
}

/** Online first, then by name whatever its case (UI audit #175). */
export function byOnlineThenName(names: string[]): string[] {
  const on = (n: string) => Number(dotOf(machineState(n)) === "on");
  return [...names].sort((a, b) => on(b) - on(a) || a.localeCompare(b, undefined, { sensitivity: "base" }));
}

/** Desktop: the shown host, opening a menu of the others. */
export function HostButton({ path = true }: { path?: boolean } = {}) {
  const ref = useRef<HTMLButtonElement>(null);
  const expanded = useMenuOpenFrom(ref);
  if (!useHosts()) return null;
  const open = (e: MouseEvent) => {
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    const groups = hostGroups();
    const items: MenuItem[] =
      groups.length > 1
        ? groups.flatMap((g) => [{ header: g.label } as MenuItem, ...byOnlineThenName(g.names).map(pickerItem)])
        : byOnlineThenName(directory.names).map(pickerItem);
    // M51: boxes reached over ssh, which only a terminal can open.
    if (directory.sshOnly.length) {
      items.push("separator", { header: "From a terminal (ssh)" } as MenuItem);
      for (const h of directory.sshOnly) {
        items.push({ label: h.name, detail: `arugula --host ${h.name} ls`, current: false, disabled: true, run: () => {} });
      }
    }
    // One description of the swarm, as the rail's (UI audit #176).
    items.push("separator", { label: "Go to Swarm", detail: SWARM_ABOUT, run: openSwarm });
    // The shown machine's own settings: here, not in the account menu.
    const mine = machineItems();
    // #778: what its place means is its row's tooltip (UI audit #175: the
    // paragraph here was the menu's longest line). M50: which way its
    // blocks through control went, as the button says for the machine's
    // own connection.
    const blocks = blockRoutesLabel();
    const means: MenuItem[] = blocks ? [{ label: blocks, hint: blocks, disabled: true, run: () => {} }] : [];
    if (mine.length || means.length) items.push("separator", { header: `This machine: ${directory.current}` } as MenuItem, ...means, ...mine);
    if (fleet?.notice) items.push("separator", { label: fleet.notice, hint: fleet.notice, disabled: true, run: () => {} });
    if (directory.stale) {
      const what = directory.control ? "Control unreachable: saved list" : "Home daemon unreachable: saved list";
      items.push("separator", { label: what, disabled: true, run: () => {} });
    }
    const all = allMachines();
    if (all.length) items.push("separator", ...all);
    // #325: whether and where this machine is joined to control.
    if (directory.shown === null && !directory.control) items.push(...controlMenuItems());
    items.push(...extras());
    openMenu({ clientX: r.left, clientY: r.bottom + 4, preventDefault: () => e.preventDefault() }, items, e.currentTarget as HTMLElement);
  };
  return (
    <button ref={ref} class="host-button" title="Hosts" aria-haspopup="menu" aria-expanded={expanded} data-host={directory.current} onClick={open} onContextMenu={open}>
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

/** How long "Connecting to …" waits before offering the others: a fresh
 * load takes 5–15 s through control, and the list read like an error page
 * (UI audit #175). */
const OFFER_OTHERS_MS = 3000;

/** While a host hasn't answered yet (it may be down): after a while, the
 * way to another, the ones online first and the offline ones marked. */
export function HostPicker() {
  const [others, setOthers] = useState(false);
  useEffect(() => {
    const t = setTimeout(() => setOthers(true), OFFER_OTHERS_MS);
    return () => clearTimeout(t);
  }, [directory.current]);
  if (!useHosts()) return null;
  const names = byOnlineThenName(directory.names.filter((n) => n !== directory.current));
  return (
    <div class="empty host-picker">
      <p>Connecting to {directory.current}…</p>
      {others && names.length > 0 && (
        <>
          <p class="dim">Taking a while? Open another machine:</p>
          {names.map((name) => {
            const off = dotOf(machineState(name)) === "off";
            return (
              <button key={name} class={off ? "faded" : undefined} data-host={name} onClick={() => directory.select(name)}>
                Switch to {name}
                {off && <span class="dim"> · {tagOf(machineState(name))}</span>}
              </button>
            );
          })}
        </>
      )}
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

/** The sheet's dot for a machine, from machineState's words: up, away, or
 * somewhere between (connecting, reconnecting). */
function dotOf(state: string): "on" | "off" | "mid" {
  if (state === "online" || state.startsWith("running")) return "on";
  if (state.startsWith("connecting") || state.startsWith("reconnecting")) return "mid";
  return "off";
}

/** The short tag that matters: "offline 8h", "asleep"; nothing when online. */
function tagOf(state: string): string {
  if (dotOf(state) === "on") return "";
  const m = /^(?:offline|reconnecting), seen (\S+) ago$/.exec(state);
  if (m) return `${state.startsWith("offline") ? "offline" : "reconnecting"} ${m[1]}`;
  return state.split(/[,·]/)[0].trim();
}

/** The phone sheet's machines (the Machines tab): one line each, in groups. */
const CLOUD = "Cloud workspaces";
const FILTER_OVER = 8;

/** Phone: a section of the sheet listing every host. */
export function HostSection({ close }: { close: () => void }) {
  const [filter, setFilter] = useState("");
  // The cloud group's own fold; undefined follows the default (shut unless
  // the current machine is in it).
  const [cloudOpen, setCloudOpen] = useState<boolean | undefined>(undefined);
  if (!useHosts()) return null;
  const isHosted = (name: string) => !!fleet?.host(name)?.hosted;
  // Hosted machines leave their person's group for the cloud one; the
  // current machine goes first in its group, its group first.
  const pin = (names: string[]) => [...names.filter((n) => n === directory.current), ...names.filter((n) => n !== directory.current)];
  const groups = hostGroups()
    .map((g) => ({ label: g.label, names: pin(g.names.filter((n) => !isHosted(n))) }))
    .filter((g) => g.names.length)
    .sort((x, y) => Number(y.names[0] === directory.current) - Number(x.names[0] === directory.current));
  const cloud = pin(directory.names.filter(isHosted));
  const q = filter.trim().toLowerCase();
  const hit = (names: string[]) => (q ? names.filter((n) => n.toLowerCase().includes(q)) : names);
  const shown = groups.map((g) => ({ ...g, names: hit(g.names) })).filter((g) => g.names.length);
  const cloudShown = hit(cloud);
  const cloudNow = cloudOpen ?? (cloud.includes(directory.current) || (!!q && cloudShown.length > 0));
  const named = shown.length > 1 || cloudShown.length > 0;
  const row = (name: string) => {
    const state = machineState(name);
    const n = fleet?.host(name)?.summary?.panes.length;
    const tag = name === directory.current ? "you" : tagOf(state);
    return (
      <button
        key={name}
        class={name === directory.current ? "sheet-host current" : "sheet-host"}
        aria-pressed={name === directory.current}
        data-host={name}
        title={fleetLabel(name)}
        onClick={() => {
          directory.select(name);
          close();
        }}
      >
        <span class="host-dot" data-dot={dotOf(state)} data-fleet={fleet?.host(name)?.state} aria-hidden="true" />
        <span class="host-name">{name}</span>
        {tag && <span class="host-state">{tag}</span>}
        {n !== undefined && (
          <span class="host-panes" aria-label={`${n} pane${n === 1 ? "" : "s"}`}>
            {n}
          </span>
        )}
        {name !== directory.current && dotOf(state) !== "on" && <span class="sr-state">{state}</span>}
      </button>
    );
  };
  return (
    <section class="sheet-hosts">
      {directory.stale && <p class="sheet-note">Saved list</p>}
      {directory.names.length > FILTER_OVER && (
        <input
          type="search"
          class="host-filter"
          placeholder="Filter machines"
          aria-label="Filter machines"
          value={filter}
          onInput={(e) => setFilter((e.currentTarget as HTMLInputElement).value)}
        />
      )}
      {shown.map((g) => (
        <Fragment key={g.label}>
          {named && <h3 class="host-group">{g.label}</h3>}
          <div class="host-list">{g.names.map(row)}</div>
        </Fragment>
      ))}
      {cloudShown.length > 0 && (
        <Fragment>
          <h3 class="host-group">
            <button class="host-fold" aria-expanded={cloudNow} data-cloud-fold onClick={() => setCloudOpen(!cloudNow)}>
              <span class="host-caret" aria-hidden="true">
                {cloudNow ? "▾" : "▸"}
              </span>
              {CLOUD}
              <span class="host-count">{cloudShown.length}</span>
              <span class="host-caveat">{HOSTED_LABEL.split(": ")[1] ?? HOSTED_LABEL}</span>
            </button>
          </h3>
          {cloudNow && <div class="host-list">{cloudShown.map(row)}</div>}
        </Fragment>
      )}
      {q && !shown.length && !cloudShown.length && <p class="sheet-note">No machine matches.</p>}
      <div class="host-list host-add">
        {[...allMachines(), ...extras()].flatMap((item) =>
          typeof item === "object" && "run" in item
            ? [
                <button
                  key={item.label}
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
