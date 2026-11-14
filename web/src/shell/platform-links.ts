// The platform page, from the shell (M153): where chat is once control has
// a platform page of its own (`/control.json`'s `platform`, M151, "The
// platform page is on app.arugula.io, not on the keys' origin"). The
// shell's buttons and links for it open it there, by id (`/chat/:room`…:
// the platform page finds the project). Since M155 the shell doesn't draw
// it: without a platform page (a control from before, or a machine on its
// own) there's no chat view here, and the daemon's bundle holds none of the
// platform page's code.
//
// Inside the app (Arugula #989) the platform is this page: its origin is
// this one, and the shell's links to it go through the app's router, in
// place, so Back and Esc climb (`setPlatformHere`).

import { desktopApp } from "../shared/desktop";

let origin: string | null = null;
/** The app's router, when the platform is this page. */
let navigate: ((path: string) => void) | null = null;

/** The platform page's origin, when control has one. */
export const platformOrigin = () => origin;

/** Where control's `/control.json` says the platform page is. */
export function setPlatformOrigin(o: string | null | undefined) {
  origin = o && /^https?:\/\/[^/]+$/.test(o) ? o : null;
  navigate = null;
}

/** The platform is this page (the app): its paths go to `go`. */
export function setPlatformHere(go: (path: string) => void, here = location.origin) {
  origin = here;
  navigate = go;
}

/** Whether the platform is this page, so its links stay in it. */
export const platformHere = () => navigate !== null;

/** The platform page at `path`: in a tab of its own beside the terminals
 * (one, reused), or in this window in the desktop app and an installed
 * app on a phone, where tabs aren't (#288); in the app, in place. Nothing
 * without a platform page. */
export function openPlatform(path: string) {
  if (!origin) return;
  if (navigate) return navigate(path);
  const url = `${origin}${path}`;
  const standalone = matchMedia?.("(display-mode: standalone)").matches;
  if (desktopApp() || standalone || !window.open(url, "arugula-platform")) location.assign(url);
}

/** The platform page's address for `path`, for a link's `href`: in the
 * app, the path itself. */
export const platformHref = (path: string) => (navigate ? path : origin ? `${origin}${path}` : undefined);
