// The sessions a person pinned (Arugula
// iss_c0fd27ac196647fa7c857dc3659a0098): which they are, and where each
// list puts them. The Sessions sidebar, the phone's sheet and the Panes
// page all read this, so they agree. A pin is a machine's id and a session
// id there; the list is the account's, kept on the platform in pin order
// (control-wire's `session_pins`).
//
// Needs you comes first and holds every session that needs you, pinned or
// not: a pinned one is in Needs you while it asks, and back in Pinned once
// answered. Pinned is the rest of the pins, oldest first. A pin whose
// session isn't there is never drawn.

import type { SessionPin } from "./proto";

/** The one word for it, everywhere. */
export const PINNED = "Pinned";

/** Pins an account may keep (control-wire's `MAX_PINS`). */
export const MAX_PINS = 100;

const is = (p: SessionPin, machine: string, session: number) => p.machine === machine && p.session === session;

export const isPinned = (pins: readonly SessionPin[], machine: string, session: number): boolean => pins.some((p) => is(p, machine, session));

/** `session` pinned last, unless it is already or the list is full. */
export function pin(pins: SessionPin[], machine: string, session: number): SessionPin[] {
  if (isPinned(pins, machine, session) || pins.length >= MAX_PINS) return pins;
  return [...pins, { machine, session }];
}

export function unpin(pins: SessionPin[], machine: string, session: number): SessionPin[] {
  return isPinned(pins, machine, session) ? pins.filter((p) => !is(p, machine, session)) : pins;
}

/** The pins without those on `machine` whose session isn't among
 * `sessions` (it closed): dropped when the list is next written. */
export function tidy(pins: SessionPin[], machine: string, sessions: Iterable<number>): SessionPin[] {
  const there = new Set(sessions);
  const kept = pins.filter((p) => p.machine !== machine || there.has(p.session));
  return kept.length === pins.length ? pins : kept;
}

/** `machine`'s pinned sessions, in pin order. */
export const pinnedOn = (pins: readonly SessionPin[], machine: string | undefined): number[] =>
  machine ? pins.filter((p) => p.machine === machine).map((p) => p.session) : [];

export type Group = "needs" | "pinned" | "working" | "idle";

/** A list's groups, in the order they're drawn: Needs you, Pinned (in pin
 * order), Working, Idle. `of` says a row's session and the group it would
 * be in unpinned. Every group is there, empty or not. */
export function sessionGroups<T>(rows: readonly T[], pinned: readonly number[], of: (r: T) => { id: number; group: Exclude<Group, "pinned"> }): { id: Group; rows: T[] }[] {
  const at = new Map(pinned.map((id, i) => [id, i]));
  const place = (r: T): Group => {
    const { id, group } = of(r);
    return group !== "needs" && at.has(id) ? "pinned" : group;
  };
  const groups = (["needs", "pinned", "working", "idle"] as const).map((id) => ({ id, rows: rows.filter((r) => place(r) === id) }));
  groups[1].rows.sort((a, b) => at.get(of(a).id)! - at.get(of(b).id)!);
  return groups;
}

/** A pinned session's place on the Panes page: its machine's group, its
 * id there, and its panes. */
export interface PinnedPanes<G extends { panes: unknown[] }> {
  pin: SessionPin;
  group: G;
  panes: G["panes"];
}

/** The Panes page's Pinned section, in pin order, and the machines' groups
 * without those panes. `groups` are already filtered as the page is: a
 * pin whose session has no pane in them isn't there. A machine whose panes
 * all went to Pinned is left out below. */
export function pinnedPanes<G extends { panes: unknown[] }>(
  groups: readonly G[],
  pins: readonly SessionPin[],
  machineOf: (g: G) => string | undefined,
  sessionOf: (p: G["panes"][number]) => number | undefined,
): { pinned: PinnedPanes<G>[]; groups: G[] } {
  const pinned: PinnedPanes<G>[] = [];
  for (const pin of pins) {
    const group = groups.find((g) => machineOf(g) === pin.machine);
    const panes = group?.panes.filter((p) => sessionOf(p) === pin.session) ?? [];
    if (group && panes.length) pinned.push({ pin, group, panes });
  }
  if (!pinned.length) return { pinned, groups: [...groups] };
  const rest = groups.flatMap((g) => {
    const mine = pinnedOn(pins, machineOf(g));
    if (!mine.length) return [g];
    const panes = g.panes.filter((p) => !mine.includes(sessionOf(p) ?? -1));
    if (panes.length === g.panes.length) return [g];
    return panes.length ? [{ ...g, panes }] : [];
  });
  return { pinned, groups: rest };
}

/** A session's state, from its panes', for its dot: it needs you if one
 * does, else working if one is, else idle. */
export function stateOf<P>(panes: readonly P[], needs: (p: P) => boolean, working: (p: P) => boolean): "needs_input" | "working" | "idle" {
  if (panes.some(needs)) return "needs_input";
  return panes.some(working) ? "working" : "idle";
}
