// The platform page, from the shell (M153): where chat is once control has
// a platform page of its own (`/control.json`'s `platform`, M151, "The
// platform page is on app.arugula.io, not on the keys' origin"). The
// shell's buttons and links for it open it there, by id (`/chat/:room`…:
// the platform page finds the project). Since M155 the shell doesn't draw
// it: without a platform page (a control from before, or a machine on its
// own) there's no chat view here, and the daemon's bundle holds none of the
// platform page's code.

import { desktopApp } from "../shared/desktop";

let origin: string | null = null;

/** The platform page's origin, when control has one. */
export const platformOrigin = () => origin;

/** Where control's `/control.json` says the platform page is. */
export function setPlatformOrigin(o: string | null | undefined) {
  origin = o && /^https?:\/\/[^/]+$/.test(o) ? o : null;
}

/** The platform page at `path`: in a tab of its own beside the terminals
 * (one, reused), or in this window in the desktop app and an installed
 * app on a phone, where tabs aren't. Nothing without a platform page. */
export function openPlatform(path: string) {
  if (!origin) return;
  const url = `${origin}${path}`;
  const standalone = matchMedia?.("(display-mode: standalone)").matches;
  if (desktopApp() || standalone || !window.open(url, "arugula-platform")) location.assign(url);
}

/** The platform page's address for `path`, for a link's `href`. */
export const platformHref = (path: string) => (origin ? `${origin}${path}` : undefined);
