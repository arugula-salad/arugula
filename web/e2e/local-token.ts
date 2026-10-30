// Loopback callers show the daemon's local token (crates/daemon/src/
// localauth.rs). Test daemons all read one file, $ARUGULA_LOCAL_TOKEN_FILE
// (made here, in a directory of its own removed at exit, and inherited by
// every daemon and worker), so they share one page key too. Browsers carry
// that key as a header on every request (`keyHeaders`, the config's
// extraHTTPHeaders: a real page sends it from its own storage, which a test
// can't seed for ports not known yet); and importing this makes Node's fetch
// send it to loopback, where only daemons read it.

import { createHash, randomBytes } from "node:crypto";
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

if (!process.env.ARUGULA_LOCAL_TOKEN_FILE) {
  const dir = mkdtempSync(join(tmpdir(), "arugula-e2e-token-"));
  process.env.ARUGULA_LOCAL_TOKEN_FILE = join(dir, "local-token");
  process.on("exit", () => rmSync(dir, { recursive: true, force: true }));
}
const file = process.env.ARUGULA_LOCAL_TOKEN_FILE!;
if (!existsSync(file)) writeFileSync(file, `ilt_${randomBytes(32).toString("hex")}`, { mode: 0o600 });

export const localToken = readFileSync(file, "utf8").trim();

/** The page key (localauth.rs `page_key`): what a daemon's page shows. */
export const pageKey = `ilp_${createHash("sha256").update(`arugula page key\0${localToken}`).digest("hex")}`;

/** For a browser context's `extraHTTPHeaders`: every daemon of the run
 * takes it. A context that sets its own headers (a tailnet identity) drops
 * it, as it should. */
export const keyHeaders = { "x-arugula-key": pageKey };

/** A daemon's sign-in link (for a browser without the key header). */
export const signIn = (origin: string, next = "/") => `${origin}/auth?token=${localToken}&next=${encodeURIComponent(next)}`;

const loopback = ["127.0.0.1", "localhost", "[::1]"];
const plain = globalThis.fetch as typeof fetch & { arugula?: true };
if (!plain.arugula) {
  const withToken = (input: Parameters<typeof fetch>[0], init?: RequestInit) => {
    const url = new URL(input instanceof Request ? input.url : String(input));
    if (!loopback.includes(url.hostname)) return plain(input, init);
    const headers = new Headers(init?.headers ?? (input instanceof Request ? input.headers : undefined));
    // A request with cookies of its own is for something else (wisp's UI).
    if (headers.has("cookie") || headers.has("x-arugula-key")) return plain(input, init);
    headers.set("x-arugula-key", pageKey);
    return plain(input, { ...init, headers });
  };
  globalThis.fetch = Object.assign(withToken, { arugula: true as const });
}
