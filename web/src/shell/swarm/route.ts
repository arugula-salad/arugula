// Where the swarm lives (M26): `/#swarm`, beside the tabs. `#swarm=N` or
// `#swarm=<daemon>.N` (a notification's deep link) opens it at pane N's card.
// `#swarm=board:<brd_id>` opens it on one board's agents, by the lane of
// their rows (#466).

/** What the swarm is, in the same words wherever it's offered: the rail,
 * the places, the machine menu, the palette (UI audit #176). */
export const SWARM_ABOUT = "every pane on all machines";

export interface SwarmRoute {
  /** A pane to show: its daemon (through control), and its id. */
  focus: { daemon?: string; pane: number } | null;
  /** A board to show the agents of (`brd_…`). */
  board: string | null;
}

const listeners = new Set<() => void>();

export function swarmRoute(): SwarmRoute | null {
  const m = /^#swarm(?:=(?:board:(brd_[\w-]+)|(?:([0-9a-f]+)\.)?(\d+)))?$/.exec(location.hash);
  if (!m) return null;
  return { focus: m[3] ? { daemon: m[2], pane: Number(m[3]) } : null, board: m[1] ?? null };
}

export function onSwarmRoute(fn: () => void): () => void {
  listeners.add(fn);
  return () => listeners.delete(fn);
}

function changed() {
  for (const fn of listeners) fn();
}
addEventListener("hashchange", changed);

export function openSwarm() {
  if (location.hash !== "#swarm") location.hash = "swarm";
}

/** The swarm on one board's agents. */
export function openSwarmBoard(board: string) {
  location.hash = `swarm=board:${board}`;
}

/** Back from a board to every pane. */
export function closeSwarmBoard() {
  location.hash = "swarm";
}

export function closeSwarm() {
  history.replaceState(null, "", location.pathname + location.search);
  changed();
}
