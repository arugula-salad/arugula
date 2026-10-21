// The platform page, from the shell (M153): where chat and Cloud
// workspaces are once control has a platform page of its own
// (`/control.json`'s `platform`, M151, "The platform page is on
// app.arugula.io, not on the keys' origin"). The shell's buttons and links
// for them open it there, by id (`/chat/:room`, `/workspaces/:id`…: the
// platform page finds the project). Without one (a control from before, or
// a machine on its own) the shell draws them itself, as before.

import { hashRoutes, setCloudRoutes, type CloudRoutes } from "../shared/cloud";
import { desktopApp } from "../shared/desktop";

let origin: string | null = null;

/** The platform page's origin, when control has one. */
export const platformOrigin = () => origin;

/** Where control's `/control.json` says the platform page is. */
export function setPlatformOrigin(o: string | null | undefined) {
  origin = o && /^https?:\/\/[^/]+$/.test(o) ? o : null;
  if (origin) setCloudRoutes(linkOut);
}

/** The platform page at `path`: in a tab of its own beside the terminals
 * (one, reused), or in this window in the desktop app and an installed
 * app on a phone, where tabs aren't. */
export function openPlatform(path: string) {
  if (!origin) return;
  const url = `${origin}${path}`;
  const standalone = matchMedia?.("(display-mode: standalone)").matches;
  if (desktopApp() || standalone || !window.open(url, "arugula-platform")) location.assign(url);
}

/** Cloud's views' routes in the shell, once there's a platform page:
 * opening a workspace, a tab or a past one goes there. Old `#cloud…`
 * links are still read, to send them there (M154). */
const linkOut: CloudRoutes = {
  ...hashRoutes,
  openWorkspace(workspace, tab) {
    openPlatform(`/workspaces${workspace ? `/${workspace}${tab ? `/${tab}` : ""}` : ""}`);
  },
  openPast(id) {
    openPlatform(`/past/${id}`);
  },
};
