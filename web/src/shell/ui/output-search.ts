// Search output (native #245): what `arugula search` and MCP's
// `history kind output` read (`GET /api/search`), from the command palette.
// This machine is asked through the page's own client, and every other
// machine of yours the page is connected to through its summary connection
// (the fleet's); each says how it went, so the palette can say who was
// asked. Picking a hit shows its pane, scrolled to the line when it is
// still in the terminal's scrollback, else at its live end with the line in
// a note.

import type { ApiResponse, Client } from "../client";
import { directory } from "../hosts";
import { getFleet } from "./hosts";
import { closePlace, currentPlace } from "./places";
import { openDoc } from "./docs-route";

/** `GET /api/search`'s hit (crates/proto/src/api.rs; not in proto.gen). */
interface SearchHit {
  pane: number;
  open: boolean;
  offset: number;
  line: string;
  command?: string | null;
  /** A synced copy of another host's history: never asked for here. */
  host?: string;
  doc?: string;
  at_ms?: number;
}

export interface OutputHit extends SearchHit {
  /** The machine it's on, by its name in the directory ("" for a page
   * with none: the one it's served from). */
  machine: string;
  /** On the machine this page shows. */
  here: boolean;
}

/** How asking one machine went. */
export interface Asked {
  machine: string;
  here: boolean;
  /** `ok` with its hit count; else why it wasn't searched. */
  state: "ok" | "error";
  hits: number;
  why?: string;
}

/** Hits per machine: `arugula search`'s default. */
const LIMIT = 50;

/** What's typed, as a regular expression for the daemon: the text itself,
 * any case (the CLI's takes a regular expression; a palette takes words). */
export const literal = (text: string) => `(?i)${text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}`;

/** Search every machine of yours this page is connected to. Another
 * person's machine isn't asked (its search is its owner's). */
export async function searchOutput(client: Client, text: string): Promise<{ hits: OutputHit[]; asked: Asked[] }> {
  const path = `/api/search?re=${encodeURIComponent(literal(text))}&limit=${LIMIT}`;
  const here = directory.current;
  const ask = async (machine: string, isHere: boolean, req: () => Promise<ApiResponse>) => {
    try {
      const res = await req();
      if (!res.ok) {
        const v = await res.json<{ error?: string }>().catch(() => null);
        return { asked: { machine, here: isHere, state: "error" as const, hits: 0, why: v?.error ?? `HTTP ${res.status}` }, hits: [] };
      }
      const list = (await res.json<SearchHit[]>()).filter((h) => !h.host);
      return {
        asked: { machine, here: isHere, state: "ok" as const, hits: list.length },
        hits: list.map((h) => ({ ...h, machine, here: isHere })),
      };
    } catch (e) {
      return { asked: { machine, here: isHere, state: "error" as const, hits: 0, why: e instanceof Error ? e.message : String(e) }, hits: [] };
    }
  };
  const others = (getFleet()?.list ?? []).filter((h) => h.name !== here && h.state === "connected" && !h.owner);
  const all = await Promise.all([
    ask(here, true, () => client.request("GET", path)),
    ...others.map((h) => ask(h.name, false, () => getFleet()!.request(h.name, "GET", path))),
  ]);
  // Newest first; a hit with no time (an agent's line) after those with one.
  const hits = all.flatMap((a) => a.hits).sort((a, b) => (b.at_ms ?? 0) - (a.at_ms ?? 0));
  return { hits, asked: all.map((a) => a.asked) };
}

/** Show a hit: its pane (on its machine), at the line where it can. */
export function openHit(client: Client, hit: OutputHit) {
  if (hit.doc && hit.here) {
    void openDoc(client, hit.doc).then((why) => why && client.toast(why));
    return;
  }
  if (!hit.open || !hit.pane) {
    client.toast("Its pane is closed");
    return;
  }
  const at = currentPlace();
  if (at !== "panes") closePlace(at);
  if (!hit.here) {
    getFleet()?.open(hit.machine, hit.pane, false, hit.line, missed(hit.line));
    return;
  }
  client.setActive(hit.pane);
  requestAnimationFrame(() => {
    client.viewOf(hit.pane)?.focus();
    const view = client.panes.get(hit.pane)?.view;
    if (!view?.reveal(hit.line)) client.toast(missed(hit.line));
  });
}

/** The note for a line the terminal no longer holds (or a block, which
 * has no scrollback to scroll). */
export const missed = (line: string) => `Not in its scrollback now: “${line.trim().slice(0, 120)}”`;
