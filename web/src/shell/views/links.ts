// How a View reaches its panes: a machine by its address, a connection to it.
// Every leaf goes through these, the machine shown too: the page's main client
// is closed when you switch machines, and has a selected tab and active pane a
// View mustn't disturb (docs/design/views.md).

import type { Client } from "../client";
import { PaneLinks } from "../blocks/pane-links";
import { directory } from "../hosts";
import { getFleet } from "../ui/hosts";
import { cleanTitle } from "../../shared/titles";
import { addressOf, parseAddress, type Address } from "./model";

/** A pane nobody shows stays drawn this long, so going from the swarm's stage
 * to its View doesn't load it again. */
const LINGER_MS = 4000;

let makeClient: ((base: string) => Client) | null = null;

/** The page says how it connects to a machine; a share link's page doesn't,
 * and has no Views. */
export function setViewClients(make: (base: string) => Client) {
  makeClient = make;
}

export const viewsAvailable = () => makeClient !== null;

/** The host's name for a machine's address part (control's device id, or the
 * name itself without control). */
export function hostOf(machine: string): string | null {
  const h = directory.list?.hosts.find((x) => x.id === machine || x.name === machine);
  if (h) return h.name;
  return machine === directory.home ? machine : null;
}

/** A pane's address: a device id survives renaming the machine. */
export function addressFor(host: string, pane: number): Address {
  return addressOf(directory.find(host)?.id ?? host, pane);
}

/** One connection per machine, keyed by its address part. */
export const viewLinks = new PaneLinks<object>((machine) => {
  const host = hostOf(machine);
  return host !== null && makeClient ? makeClient(directory.base(host)) : null;
}, LINGER_MS);

/** A leaf's name without opening it, for what's beside the tabs: what the
 * fleet's summaries say. */
export function paneTitle(a: Address): string {
  const at = parseAddress(a);
  if (!at) return a;
  const host = hostOf(at.machine);
  const p = host === null ? undefined : getFleet()?.panes.find((x) => x.host === host && x.id === at.pane);
  const live = viewLinks.client(at.machine);
  return (live?.title(at.pane) || (p && cleanTitle(p.info.title ?? p.info.command ?? "")) || `%${at.pane}`).trim();
}
