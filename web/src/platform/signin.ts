// Signing in to the platform page (M152, "The hub signs people in to the
// platform page"). A visitor without a session goes to the hub's
// `/signin/platform`, which uses the hub's own session (or signs them in
// there, passkeys and all) and sends the browser back here with a one-time
// code in the fragment: short-lived, single use, bound to this address and
// to a PKCE verifier only this page holds (sessionStorage). The page swaps
// the code at the hub, on this host, for a session cookie of this host's
// alone. Someone signed in on the hub sees no prompt.

import { platformFetch, refusalOf } from "../shared/maintenance";

const KEPT = "arugula.platform-signin";
/** Signed out of this page on purpose: no hop by itself until they ask. */
const OUT = "arugula.platform-signed-out";
/** A hop that came back without a session within this long isn't tried
 * again by itself: the page says where to sign in instead. */
const AGAIN_MS = 30_000;

interface Kept {
  verifier: string;
  state: string;
  at: number;
}

const b64url = (b: Uint8Array) => btoa(String.fromCharCode(...b)).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
const random = (n: number) => b64url(crypto.getRandomValues(new Uint8Array(n)));

async function challengeOf(verifier: string): Promise<string> {
  return b64url(new Uint8Array(await crypto.subtle.digest("SHA-256", new TextEncoder().encode(verifier))));
}

/** This address without its fragment: what the code is bound to. */
const here = () => location.origin + location.pathname + location.search;

function kept(): Kept | null {
  try {
    return JSON.parse(sessionStorage.getItem(KEPT) ?? "null") as Kept | null;
  } catch {
    return null;
  }
}

/** Whether to leave the visitor be: a hop was tried a moment ago (and so
 * came back without a session), or they signed this page out. */
export function triedJustNow(): boolean {
  const k = kept();
  let out = false;
  try {
    out = sessionStorage.getItem(OUT) !== null;
  } catch {
    // no storage: nothing kept
  }
  return out || (!!k && Date.now() - k.at < AGAIN_MS);
}

/** Off to the hub to sign in, back here after. */
export async function signIn(control: string) {
  const k: Kept = { verifier: random(32), state: random(16), at: Date.now() };
  try {
    sessionStorage.removeItem(OUT);
    sessionStorage.setItem(KEPT, JSON.stringify(k));
  } catch {
    // Without storage the code can't be swapped: the hop would loop.
    return false;
  }
  const q = new URLSearchParams({ return: here(), challenge: await challengeOf(k.verifier), state: k.state });
  location.replace(`${control}/signin/platform?${q}`);
  return true;
}

/** Back from the hub with a code: swap it for this host's session. The
 * code leaves the address at once, used or not. */
export async function finishSignIn(): Promise<boolean> {
  const m = /^#code=([0-9a-f]{64})&state=([A-Za-z0-9_-]{16,64})$/.exec(location.hash);
  if (!m) return false;
  history.replaceState(null, "", location.pathname + location.search);
  const k = kept();
  if (!k || k.state !== m[2]) return false;
  const r = await platformFetch("/api/app/session", {
    method: "POST",
    credentials: "same-origin",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ code: m[1], verifier: k.verifier, return: here() }),
  });
  if (!r.ok) {
    console.warn(`signing in here was refused: ${refusalOf(r.status, await r.json().catch(() => ({})))}`);
    return false;
  }
  return true;
}

/** Sign this page out (the hub's session stays), and stay out until the
 * visitor signs in again here. */
export async function signOut() {
  try {
    sessionStorage.setItem(OUT, "1");
  } catch {
    // without storage the next visit signs in again by itself
  }
  await platformFetch("/api/app/signout", { method: "POST", credentials: "same-origin" }).catch(() => {});
}
