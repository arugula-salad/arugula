// The push hint's fact: something wanted this person where this device
// can be notified but isn't: a pane or an agent went needs-you while the
// tab was hidden, or one waits on any machine now. Never where push can't work
// (plain http, a browser without it, notifications blocked, an iPhone's
// tab before Add to Home Screen) or where it's on already: only "off".
// Not on a machine with no account either: the phone offer (./phone) is
// that moment's there.

import type { Client } from "../client";
import type { Fact } from "./index";
import { factChanged } from "./facts";
import { pushNow, subscribePush } from "../ui/notify";
import { directory } from "../hosts";
import { getFleet } from "../ui/hosts";
import { needsYou } from "../../shared/needs-you";
import { controlState, subscribeControlState } from "../ui/control-state";

/** Something went needs-you while the page was hidden. */
let missed = false;
let watching = false;

export function pushWanted(client: Client): Fact | null {
  if (!watching) {
    watching = true;
    subscribePush(factChanged);
    subscribeControlState(factChanged);
    document.addEventListener("visibilitychange", factChanged);
  }
  if (pushNow() !== "off" || controlState()?.state === "not_joined") return null;
  if (document.visibilityState === "hidden") {
    missed ||= !!client.state?.panes.some((p) => p.attention === "needs_input");
    // Said when they're back.
    return null;
  }
  return missed || waitingCount(client) > 0 ? { args: {} } : null;
}

/** How many need you, on every machine: the shown one's panes and notices
 * from its own state, the others' from the fleet's summaries. */
function waitingCount(client: Client): number {
  const current = directory.current;
  const fleet = getFleet();
  const here = (client.state?.panes ?? []).filter(needsYou).length + (client.state?.notices?.length ?? 0);
  const panes = (fleet?.panes ?? []).filter((p) => p.host !== current && needsYou(p.info)).length;
  const notices = (fleet?.list ?? []).filter((h) => h.name !== current).reduce((n, h) => n + (h.summary?.notices?.length ?? 0), 0);
  return here + panes + notices;
}
