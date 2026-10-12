// What the bootstrap page and the shim share: asking the parent page
// (control's page, which framed this block) for the block's credentials,
// and handing them to the worker. The messages are the contract in
// docs/control-e2e.md ("The frame's contract with control's page");
// control's page answers them (PR 4 of #150).

import type { Creds, Held, Route } from "./channel.ts";

/** Every message of the contract carries this tag and version. */
export const TAG = "arugula-block";
export const VERSION = 1;

/** From the frame to the parent. */
export interface Ask {
  arugula: typeof TAG;
  v: typeof VERSION;
  ask: "hello" | "renew";
}

/** From the frame to the parent: it can't load the block, and why. The
 * parent falls back to the daemon's own block site where there is one. */
export interface Failed {
  arugula: typeof TAG;
  v: typeof VERSION;
  failed: string;
}

/** From the frame to the parent: which way the block's channel actually
 * went. A direct route that doesn't connect falls back to the relay
 * without asking, so the route in its credentials isn't the answer. The
 * block's own pages could say anything here: it's for showing only. */
export interface Routed {
  arugula: typeof TAG;
  v: typeof VERSION;
  route: Route;
}

/** From the parent to the frame: credentials, or why there are none. */
export type Answer = { arugula: typeof TAG; v: typeof VERSION } & ({ creds: Creds } | { refused: string });

/** Control's origins, which may frame a block, from the bootstrap page's
 * `<meta name=arugula-control>` (control's blocks.rs), or the parent a
 * block already knows. */
export function controlOrigins(doc: Document): string[] {
  const m = doc.querySelector('meta[name="arugula-control"]')?.getAttribute("content") ?? "";
  return m.split(/\s+/).filter(Boolean);
}

/** Ask the parent page for credentials. The message goes to each of
 * control's origins (only the parent's own receives it); an answer counts
 * only from the parent window at one of them. */
export function askParent(origins: string[], ask: Ask["ask"], timeoutMs = 10_000): Promise<Held> {
  return new Promise((res, rej) => {
    if (parent === window) return rej(new Error("a block opens inside Arugula control's page"));
    const done = (f: () => void) => {
      clearTimeout(t);
      removeEventListener("message", on);
      f();
    };
    const t = setTimeout(() => done(() => rej(new Error("the page around this block didn't answer"))), timeoutMs);
    const on = (e: MessageEvent) => {
      const a = e.data as Partial<Answer> | null;
      if (e.source !== parent || !origins.includes(e.origin) || a?.arugula !== TAG || a.v !== VERSION) return;
      if ("creds" in a && a.creds) {
        const creds = a.creds;
        done(() => res({ creds, parent: e.origin }));
      } else done(() => rej(new Error("refused" in a && a.refused ? a.refused : "no credentials for this block")));
    };
    addEventListener("message", on);
    const m: Ask = { arugula: TAG, v: VERSION, ask };
    for (const o of origins) parent.postMessage(m, o);
  });
}

/** Tell the parent this block can't load here. */
export function tellFailed(origins: string[], why: string) {
  if (parent === window) return;
  const m: Failed = { arugula: TAG, v: VERSION, failed: why };
  for (const o of origins) parent.postMessage(m, o);
}

/** Tell the parent which way the block's channel went: to the origin
 * that answered, from `Held.parent`. */
export function tellRoute(parentOrigin: string, route: Route) {
  if (parent === window) return;
  const m: Routed = { arugula: TAG, v: VERSION, route };
  parent.postMessage(m, parentOrigin);
}

/** Give the worker the block's credentials; resolves once it has them. */
export async function giveWorker(held: Held): Promise<void> {
  const reg = await navigator.serviceWorker.ready;
  const sw = reg.active;
  if (!sw) throw new Error("the block's worker isn't running");
  await new Promise<void>((res, rej) => {
    const ch = new MessageChannel();
    const t = setTimeout(() => rej(new Error("the block's worker didn't take its key")), 10_000);
    ch.port1.onmessage = () => {
      clearTimeout(t);
      res();
    };
    sw.postMessage({ arugula: TAG, v: VERSION, held }, [ch.port2]);
  });
}
