// The footer under the workspace. M108 (#1023) fills it: the host, repo and
// branch when known, the agents running and what needs you, with hints from
// ui/keys.ts. Today it's the connection's status pill only.

import type { Client } from "../../client";

/** The footer itself: nothing yet (M108). */
export function StatusFooter(_: { client: Client }) {
  return null;
}

export function StatusPill({ client }: { client: Client }) {
  const text = client.error ?? (client.connected ? null : client.state ? "reconnecting…" : "connecting…");
  if (!text) return null;
  return (
    <div id="status" role="status" class={client.error ? "error" : ""}>
      {text}
    </div>
  );
}
