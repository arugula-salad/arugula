// Control's pages move to the app (Arugula #989, decision 11): once the
// cloud says so, this page's own addresses (a pane, a machine, Swarm, a
// machine's own places) open at their places on app.arugula.io, forever.
// Signing in, passkeys, your account and devices, secrets in fragments,
// deploy approvals and chat stay here: `fromControl` (shared/machine-paths)
// says which is which.
//
// The switch is the cloud's: `pages: "app"` in /control.json, with
// `platform` the app's origin. control.ts's `ControlInfo` doesn't name
// `pages`; it's read from the same answer, as it came.

import { fromControl } from "../shared/machine-paths.ts";

/** What /control.json says about where its pages are. */
export interface PagesFlag {
  pages?: unknown;
  platform?: string | null;
}

/** Whether control's pages live in the app now, and its origin. */
export function appOrigin(info: PagesFlag | null): string | null {
  if (!info || info.pages !== "app" || typeof info.platform !== "string") return null;
  return /^https?:\/\/[^/]+$/.test(info.platform) ? info.platform : null;
}

/** Where this page's address (`hash`, `search`) is in the app, or null
 * when it stays here. Bare (`/`), it moves only for someone signed in:
 * signed out, this is where they sign in. */
export function appAddress(info: PagesFlag | null, hash: string, search: string, signedIn: boolean): string | null {
  const origin = appOrigin(info);
  const to = origin ? fromControl(hash, search) : null;
  if (!origin || !to || (to === "/" && !signedIn)) return null;
  return `${origin}${to}`;
}
