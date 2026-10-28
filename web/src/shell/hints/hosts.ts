// The other machine's hint's fact: the host menu lists a machine besides
// this one (the home daemon's list, `remoteHosts`), and the shown tab has
// a terminal: a shell there can split into this tab beside it.

import type { Client } from "../client";
import type { Fact } from "./index";
import { factChanged } from "./facts";
import { remoteHosts } from "../blocks";
import { directory } from "../hosts";

let watching = false;

export function otherHost(client: Client): Fact | null {
  if (!watching) {
    watching = true;
    directory.subscribe(factChanged);
  }
  if (client.state?.roles) return null;
  const host = remoteHosts()[0];
  const pane = client.active();
  if (!host || pane === undefined || client.info(pane)?.type !== "terminal") return null;
  return { pane, args: { pane, host } };
}
