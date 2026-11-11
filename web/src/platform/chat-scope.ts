// Which team's chat the platform page counts and lists (Arugula #883): the
// team of the project the route is in, else the current project's
// (./current-project). Personal has no team's chat, and nothing is current
// before a project's been opened: those count every room, as before.

type Owned = { team?: { id: string } | null };

/** The team whose rooms to count: `route`'s project's (`null`, the rooms of
 * no team, for the reader's own), else `current`'s team, else every room
 * (`undefined`). */
export function chatTeam(route: Owned | undefined, current: Owned | undefined): string | null | undefined {
  if (route) return route.team?.id ?? null;
  return current?.team?.id;
}
