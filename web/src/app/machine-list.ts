// The Machines page's model (Arugula #1346): each machine, its sessions in
// the project, and each session's tabs and panes, in the order the page
// draws them. Pure, so a test reads it without a page.

import type { FleetHost, FleetPane } from "../shell/fleet";
import { needsYou } from "../shared/needs-you.ts";
import { stateOf } from "../shared/session-pins.ts";
import type { ProjectRef, SessionPin } from "../shared/proto.gen";

/** Whether a session in `project`, on a machine that's `yours` or not,
 * shows here (`inShown`, Arugula #1164); every one by default. */
export type Shows = (project: ProjectRef | null | undefined, yours: boolean) => boolean;
const every: Shows = () => true;

/** One of a session's tabs, its panes in layout order. */
export interface SessionTab {
  id: number;
  name: string | null;
  panes: FleetPane[];
}

/** A session's card. */
export interface SessionCard {
  id: number;
  name: string;
  tabs: SessionTab[];
  panes: number;
  state: "needs_input" | "working" | "idle";
}

/** A machine's section. */
export interface MachineSection {
  name: string;
  id?: string;
  online: boolean;
  /** A cloud workspace, not someone's computer. */
  cloud: boolean;
  /** Whose it is when it isn't yours. */
  owner?: string;
  /** The computer this page runs on. */
  here: boolean;
  /** Its sessions in the project, needs you first. */
  sessions: SessionCard[];
}

/** A pinned session's card, with its machine. */
export interface PinnedCard {
  pin: SessionPin;
  machine: MachineSection;
  session: SessionCard;
}

/** A host's sessions that `shows` lets through, each with the panes the
 * fleet has of it (another's private pane isn't one) and `only` keeps;
 * a session left with none isn't there. */
function sessionsOf(h: FleetHost, panes: FleetPane[], shows: Shows, only: (p: FleetPane) => boolean): SessionCard[] {
  const st = h.summary;
  if (!st) return [];
  const yours = !h.owner;
  const byId = new Map(panes.filter((p) => p.host === h.name).map((p) => [p.id, p]));
  const cards: SessionCard[] = [];
  for (const s of st.sessions) {
    if (!shows(s.project, yours)) continue;
    const tabs = s.tabs.flatMap((tid): SessionTab[] => {
      const t = st.tabs.find((x) => x.id === tid);
      if (!t) return [];
      const ps = t.layout.panes.flatMap(([id]) => byId.get(id) ?? []).filter(only);
      return ps.length ? [{ id: t.id, name: t.name, panes: ps }] : [];
    });
    const all = tabs.flatMap((t) => t.panes);
    if (!all.length) continue;
    cards.push({ id: s.id, name: s.name, tabs, panes: all.length, state: stateOf(all, (p) => needsYou(p.info), (p) => p.info.attention === "working") });
  }
  return cards.sort((a, b) => Number(b.state === "needs_input") - Number(a.state === "needs_input") || a.id - b.id);
}

export interface Options {
  shows?: Shows;
  /** The name of the computer this page runs on, when it knows. */
  here?: string | null;
  /** The machine you last used (its id, or its name where it has none):
   * first of yours when there's no `here`. */
  recent?: string | null;
  /** The panes shown (Agents only's); every one by default. */
  only?: (p: FleetPane) => boolean;
  /** Whether a machine with no session in the project is listed at all
   * (yours, the project's cloud workspaces); one with some always is. */
  keep?: (h: FleetHost) => boolean;
}

/** The machines with sessions in the project, in order: this computer,
 * then online before offline, yours before teammates', the one you used
 * last, then by name; and `quiet`, those kept with none, by name. */
export function machineSections(hosts: FleetHost[], panes: FleetPane[], o: Options = {}): { machines: MachineSection[]; quiet: MachineSection[] } {
  const { shows = every, here = null, recent = null, only = () => true, keep = () => true } = o;
  const all = hosts.map(
    (h): MachineSection => ({
      name: h.name,
      id: h.id,
      online: h.status ? h.status !== "offline" : h.state === "connected",
      cloud: !!h.hosted,
      owner: h.owner,
      here: !!here && h.name === here,
      sessions: sessionsOf(h, panes, shows, only),
    }),
  );
  const last = (m: MachineSection) => !!recent && (m.id ?? m.name) === recent;
  const machines = all
    .filter((m) => m.sessions.length)
    .sort(
      (a, b) =>
        Number(b.here) - Number(a.here) ||
        Number(b.online) - Number(a.online) ||
        Number(!b.owner) - Number(!a.owner) ||
        Number(last(b)) - Number(last(a)) ||
        a.name.localeCompare(b.name),
    );
  const quiet = all.filter((m, i) => !m.sessions.length && keep(hosts[i])).sort((a, b) => a.name.localeCompare(b.name));
  return { machines, quiet };
}

/** The sessions you pinned, in pin order, and the machines without them;
 * a machine whose sessions were all pinned is left out below. `idOf`
 * says a machine's id as pins name it. */
export function pinnedCards(machines: MachineSection[], pins: readonly SessionPin[], idOf: (m: MachineSection) => string | undefined): { pinned: PinnedCard[]; machines: MachineSection[] } {
  const pinned: PinnedCard[] = [];
  for (const pin of pins) {
    const machine = machines.find((m) => idOf(m) === pin.machine);
    const session = machine?.sessions.find((s) => s.id === pin.session);
    if (machine && session) pinned.push({ pin, machine, session });
  }
  if (!pinned.length) return { pinned, machines };
  const rest = machines.flatMap((m) => {
    const sessions = m.sessions.filter((s) => !pinned.some((p) => p.machine === m && p.session === s));
    if (sessions.length === m.sessions.length) return [m];
    return sessions.length ? [{ ...m, sessions }] : [];
  });
  return { pinned, machines: rest };
}

/** "3 machines · 5 sessions · 9 panes · 1 needs you". */
export function statsLine(machines: number, cards: SessionCard[]): string {
  const n = (k: number, one: string, many: string) => `${k} ${k === 1 ? one : many}`;
  const panes = cards.reduce((k, s) => k + s.panes, 0);
  const asking = cards.reduce((k, s) => k + s.tabs.reduce((j, t) => j + t.panes.filter((p) => needsYou(p.info)).length, 0), 0);
  const parts = [n(machines, "machine", "machines"), n(cards.length, "session", "sessions"), n(panes, "pane", "panes")];
  if (asking) parts.push(`${asking} ${asking === 1 ? "needs" : "need"} you`);
  return parts.join(" · ");
}
