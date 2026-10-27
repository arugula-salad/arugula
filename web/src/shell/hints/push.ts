// The push hint's fact: something wanted this person where this device
// can be notified but isn't: a pane or an agent went needs-you while the
// tab was hidden, or the bell counts one now. Never where push can't work
// (plain http, a browser without it, notifications blocked, an iPhone's
// tab before Add to Home Screen) or where it's on already: only "off".

import type { Client } from "../client";
import type { Fact } from "./index";
import { factChanged } from "./facts";
import { pushNow, subscribePush } from "../ui/notify";
import { waitingCount } from "../ui/shell/attention-drawer";

/** Something went needs-you while the page was hidden. */
let missed = false;
let watching = false;

export function pushWanted(client: Client): Fact | null {
  if (!watching) {
    watching = true;
    subscribePush(factChanged);
    document.addEventListener("visibilitychange", factChanged);
  }
  if (pushNow() !== "off") return null;
  if (document.visibilityState === "hidden") {
    missed ||= !!client.state?.panes.some((p) => p.attention === "needs_input");
    // Said when they're back.
    return null;
  }
  return missed || waitingCount(client) > 0 ? { args: {} } : null;
}
