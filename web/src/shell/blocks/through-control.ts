// Blocks through control (M50, #150): control's page frames a port or
// editor block on its block origin, `https://b-<site_key>.<block domain>`,
// and answers that frame's asks for credentials. This is the parent's side
// of "The frame's contract with control's page" (docs/control-e2e.md); the
// frame's side is ../block/.
//
// - A frame is answered only when the message comes from a frame this page
//   made (`event.source`), at the origin it gave that frame
//   (`event.origin`), and only for that frame's block.
// - Each answer is a new X25519 key and a grant for it, signed with this
//   device's key. Its expiry is by the daemon's clock, from the device
//   channel's message 2 (`{"now"}`), a few minutes ahead and never more
//   than ten. Nothing the frame says sets a time.
// - The route is the relay, or the daemon's own `wss://…/e2e` when this page
//   reached it directly, except in WebKit (Safari, iOS, the desktop app's
//   WebKitGTK), which stays on the relay until its direct path is measured
//   (S27).
// - The frame is allowed Chrome's Local Network Access (`LNA_ALLOW`), so its
//   pages' own channel can take a direct route; its worker can't in Chrome
//   and goes through the relay (#849).
// - The frame says which way its channels actually went (`route`, and why
//   not direct), since a direct route that doesn't connect falls back to
//   the relay unasked. That's for showing only (block-route.ts).

import { hex } from "../e2e/cert.ts";
import { daemonNow, type DaemonClock, type DaemonRef } from "../e2e/channel.ts";
import { signText, type DeviceKeys } from "../e2e/keys.ts";
import { newKeyPair, publicRaw } from "../e2e/noise.ts";
import type { Creds, Grant, Route } from "../block/channel.ts";
import { TAG, VERSION } from "../block/page.ts";

/** How far ahead a grant's expiry is put, by the daemon's clock. The
 * daemon refuses more than ten minutes (`arugula_e2e::block::MAX_LIFE_MS`);
 * this leaves room for the clock estimate to be off. */
export const GRANT_LIFE_MS = 5 * 60 * 1000;
export const MAX_LIFE_MS = 10 * 60 * 1000;

/** What a block's frame is allowed beyond its block's own `allow`: Chrome's
 * Local Network Access (#849). A tailnet's 100.64.0.0/10 and a LAN are
 * "local", and Chrome checks WebSockets too since 147, so a direct route
 * from a cross-origin frame needs the permissions policy (its default is
 * 'self'); the frame's requests are then asked on this page's behalf,
 * which already has the permission when its own channel went direct.
 * `local-network` is the spec's name; `local-network-access` the older
 * one, all Chrome had before 145 and still an alias. Not
 * `loopback-network`: a daemon's direct route is never this device. The
 * worker's requests aren't covered (`workerDirectBlocked`). */
export const LNA_ALLOW = "local-network; local-network-access";

/** A frame's `allow`: the block's own plus `LNA_ALLOW`. */
export function frameAllow(own: string | undefined): string {
  return own ? `${own}; ${LNA_ALLOW}` : LNA_ALLOW;
}

/** A block's origin on control's block domain. Control serves it on the
 * same listener as its page, so the scheme and port are the page's: https
 * and no port when hosted, `http://…:7690` for a local control. */
export function blockOrigin(siteKey: string, domain: string, page: { protocol: string; port: string }): string {
  return `${page.protocol}//b-${siteKey}.${domain}${page.port ? `:${page.port}` : ""}`;
}

/** The daemon's `src` for a block, on `origin` instead: its path and query,
 * with every `vscode-remote://<host>` in an editor's `payload` (the file to
 * open, on the window's remote, which is its page's host) given the block
 * origin's host. The daemon builds `src` for its own block site, or as a
 * path when it has none; which origin a block is shown on is this page's
 * choice. */
export function blockSrc(src: string, origin: string): string {
  const u = new URL(src, origin);
  const at = new URL(origin);
  const payload = u.searchParams.get("payload");
  if (payload !== null) u.searchParams.set("payload", payload.replace(/vscode-remote:\/\/[^/"]*/g, `vscode-remote://${at.host}`));
  return `${at.origin}${u.pathname}${u.search}${u.hash}`;
}

/** Whether this browser is WebKit: Safari, every browser on iOS, and the
 * desktop app's WebKitGTK. */
export function isWebKit(ua: string, touchMac = false): boolean {
  if (/iPhone|iPad|iPod/.test(ua) || touchMac) return true;
  return /AppleWebKit/.test(ua) && !/Chrome|Chromium|Edg\//.test(ua);
}

/** `arugula_e2e::block::Grant::body`. Frozen once shipped (frozen.rs). */
export function grantBody(g: Omit<Grant, "sig">): string {
  return `arugula block grant v1\ndaemon ${g.daemon}\nblock ${g.block}\nkey ${g.key}\nexpires ${g.expires}\nby ${g.by}\n`;
}

/** A grant's expiry: `GRANT_LIFE_MS` past the daemon's now. */
export function grantExpiry(daemonNowMs: number): number {
  return Math.round(daemonNowMs) + Math.min(GRANT_LIFE_MS, MAX_LIFE_MS);
}

export interface Mint {
  keys: DeviceKeys;
  daemon: DaemonRef;
  block: string;
  /** The daemon's now, ms (`daemonNow` of the device channel's clock). */
  now: number;
  route: string;
}

/** A new block key and a grant for it. */
export async function mint(m: Mint): Promise<Creds> {
  const kp = await newKeyPair("X25519", true, ["deriveBits"]);
  const pkcs8 = await crypto.subtle.exportKey("pkcs8", kp.privateKey);
  const pub = hex(await publicRaw(kp.publicKey));
  const body = { daemon: m.daemon.id, block: m.block, key: pub, expires: grantExpiry(m.now), by: m.keys.id };
  const grant: Grant = { ...body, sig: await signText(m.keys, grantBody(body)) };
  return { daemon: { id: m.daemon.id, noise: m.daemon.noise }, key: { pkcs8, pub }, grant, route: m.route };
}

/** What a frame this page made stands for. */
export interface Framed {
  /** The `<iframe>`: its `contentWindow` is the only source answered. */
  frame: { contentWindow: unknown };
  /** The origin this page gave it: the only one answered, and the only
   * one answers go to. */
  origin: string;
  /** Credentials for its block, and nothing else. */
  creds(): Promise<Creds>;
  /** The frame couldn't load the block (its bootstrap said why). */
  failed(why: string): void;
  /** Which way the block's channel went, by the frame's word. */
  routed(how: Routing): void;
}

/** Which way a block's channel went, by its frame's word: `why` it isn't
 * direct when its route was, and `ws` when it's the pages' own channel
 * (their WebSockets) rather than the worker's (#849). */
export interface Routing {
  route: Route;
  why?: string;
  ws?: boolean;
}

/** What a frame may say. */
export type Said = { ask: "hello" | "renew" } | { failed: string } | Routing;

/** Which frame a message is from, and what it says, if it's one to act on:
 * from a frame `frames` holds (by `source`), at the origin it was given,
 * tagged and versioned per the contract. Anything else is nothing. */
export function heard(frames: Iterable<Framed>, e: { source: unknown; origin: string; data: unknown }): { from: Framed; said: Said } | null {
  const d = e.data as { arugula?: unknown; v?: unknown; ask?: unknown; failed?: unknown; route?: unknown; why?: unknown; ws?: unknown } | null;
  if (!d || typeof d !== "object" || d.arugula !== TAG || d.v !== VERSION || !e.source) return null;
  for (const f of frames) {
    if (f.frame.contentWindow !== e.source) continue;
    if (f.origin !== e.origin) return null;
    if (d.ask === "hello" || d.ask === "renew") return { from: f, said: { ask: d.ask } };
    if (typeof d.failed === "string") return { from: f, said: { failed: d.failed.slice(0, 300) } };
    if (d.route === "direct" || d.route === "relayed") {
      const said: Routing = { route: d.route };
      if (typeof d.why === "string" && d.why) said.why = d.why.slice(0, 300);
      if (d.ws === true) said.ws = true;
      return { from: f, said };
    }
    return null;
  }
  return null;
}

/** The frames this page made, answered from one listener. */
const frames = new Set<Framed>();
let listening = false;

function listen() {
  if (listening) return;
  listening = true;
  addEventListener("message", (e: MessageEvent) => {
    const h = heard(frames, e);
    if (!h) return;
    const { from, said } = h;
    if ("failed" in said) return from.failed(said.failed);
    if ("route" in said) return from.routed(said);
    const win = from.frame.contentWindow as Window | null;
    void from.creds().then(
      (creds) => win?.postMessage({ arugula: TAG, v: VERSION, creds }, from.origin),
      (err: unknown) => win?.postMessage({ arugula: TAG, v: VERSION, refused: err instanceof Error ? err.message : String(err) }, from.origin),
    );
  });
}

/** Answer `f`'s frame from now on; the returned function stops it. */
export function frameBlock(f: Framed): () => void {
  listen();
  frames.add(f);
  return () => void frames.delete(f);
}

/** What a client to the block's daemon gives for a grant. */
export interface BlockClient {
  /** Control's block domain, when this page reaches the daemon through a
   * control that serves block origins. */
  blockDomain(): string | null;
  e2e?: { daemon: DaemonRef; keys: DeviceKeys };
  /** The daemon's clock, from this page's channel to it. */
  daemonClock(): DaemonClock | undefined;
  /** The daemon's own `wss://…/e2e`, when this page reached it directly. */
  directE2E(): string | null;
}

/** Credentials for block `block` on `c`'s daemon. Refusals are sentences
 * the frame shows. */
export async function credsFor(c: BlockClient, block: string, ua = navigator.userAgent, touchMac = isTouchMac()): Promise<Creds> {
  if (!c.e2e) throw new Error("blocks open through Control");
  const clock = c.daemonClock();
  if (!clock) throw new Error("this machine isn't connected, or its Arugula is too old to show blocks here: update it");
  const direct = c.directE2E();
  const route = direct && !isWebKit(ua, touchMac) ? direct : "relay";
  return mint({ keys: c.e2e.keys, daemon: c.e2e.daemon, block, now: daemonNow(clock), route });
}

function isTouchMac(): boolean {
  // iPadOS asks for the desktop site as a Mac.
  return typeof navigator !== "undefined" && /Macintosh/.test(navigator.userAgent) && navigator.maxTouchPoints > 1;
}
