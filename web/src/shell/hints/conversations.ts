// The conversations hint's fact: Claude Code has had a conversation on
// this machine (the daemon's index of ~/.claude, `conversations.list`,
// what `arugula claude ls` reads), and none is open here: no agent block,
// no terminal running Claude Code. Asked once a page, and only while the
// hint is unseen with Tips on (./index asks only then).

import type { Client } from "../client";
import { OPS, opPath } from "../ops";
import type { Fact } from "./index";
import { factChanged } from "./facts";

/** Whether there is one: unknown until answered. */
const known = new WeakMap<Client, boolean | "asking">();

export function conversationHere(client: Client): Fact | null {
  const st = client.state;
  // The owner's.
  if (!st || st.roles) return null;
  if (st.panes.some((p) => p.type === "agent" || p.kind === "agent")) return null;
  const k = known.get(client);
  if (k === undefined) {
    known.set(client, "asking");
    void client
      .request(OPS["conversations.list"].method, `${opPath("conversations.list")}?limit=1`)
      .then(async (res) => (res.ok ? (await res.json<{ conversations: unknown[] }>()).conversations.length > 0 : false))
      .catch(() => false)
      .then((any) => {
        known.set(client, any);
        factChanged();
      });
  }
  return k === true ? { args: {} } : null;
}
