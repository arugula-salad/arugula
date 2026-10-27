// The team hint's fact: from what control tells the page about teams
// (ControlSession.teams): a team this person owns that's new (only them)
// or whose second member just joined. Inviting with a link and the
// team's chat are in Teams…

import type { Client } from "../client";
import type { ControlSession } from "../control";
import type { Fact } from "./index";
import { factChanged } from "./facts";
import { getControlSession } from "../ui/people";

const watched = new WeakSet<ControlSession>();

export function smallTeam(_client: Client): Fact | null {
  const s = getControlSession();
  if (!s) return null;
  if (!watched.has(s)) {
    watched.add(s);
    s.subscribe(factChanged);
  }
  const t = s.teams.find((t) => t.role === "owner" && t.roster.members.length <= 2);
  return t ? { args: { team: t.team } } : null;
}
