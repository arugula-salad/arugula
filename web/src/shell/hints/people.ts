// The second person's fact: someone else is in the shown session now (the
// people the top bar draws, ui/people.tsx), so following them, or typing
// together, is something to do.

import type { Client } from "../client";
import type { Fact } from "./index";

export function secondPerson(client: Client): Fact | null {
  const tabs = new Set(client.state?.sessions.find((s) => s.id === client.session)?.tabs ?? []);
  const p = client.others().find((o) => o.tab !== undefined && tabs.has(o.tab));
  return p ? { args: { client: p.client } } : null;
}
