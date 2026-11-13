// A grant card's answers (#578). Both are the owner's own calls to control,
// over their session: the machine that raised the card never grants itself.

import { platformFetch } from "./maintenance";
import { grantScope } from "./notice";
import type { Notice } from "./proto";

async function owners(path: string, body: unknown): Promise<void> {
  const r = await platformFetch(path, {
    method: "POST",
    credentials: "same-origin",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
  if (!r.ok) {
    const v = (await r.json().catch(() => ({}))) as { error?: string };
    throw new Error(v.error ?? `Request failed (${r.status})`);
  }
}

/** Grant: the owner's own `POST /api/grants`, over their control session
 * (never the machine's): `threads.read` for the machine over the room's
 * scope. Control then delivers what waited, once. */
export function grantRead(n: Notice): Promise<void> {
  const g = n.grant;
  if (!g) return Promise.reject(new Error("no grant to make"));
  const scope = grantScope(g.scope_kind, g.scope_id);
  if (!scope) return Promise.reject(new Error(`can't grant over a ${g.scope_kind}`));
  return owners("/api/grants", { daemon: g.machine, power: "threads.read", scope });
}

/** Never for this room: also the owner's, over their control session. */
export function grantNever(n: Notice): Promise<void> {
  if (!n.grant || !n.room) return Promise.reject(new Error("no room to refuse"));
  return owners(`/api/chat/rooms/${encodeURIComponent(n.room)}/grant-never`, { daemon: n.grant.machine });
}
