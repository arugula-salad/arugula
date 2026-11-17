// The Settings page's route (Arugula #695): `#settings`, or `#settings=<id>`
// at one section. In its own file so a menu can open it without the page.
// Where the app's Settings is (control's page, the app), Settings opens
// there, and this page keeps only Phones and browsers (Arugula #1110).

import { settingsPath } from "../../shared/machine-paths";
import { openPlatform, platformOrigin } from "../platform-links";

/** Sections that became one (Arugula #1108): an old link lands on it. */
const MERGED: Record<string, string> = { device: "push", "all-devices": "push" };

export function settingsRoute(): { section?: string } | null {
  const m = /^#settings(?:=([\w-]+))?$/.exec(location.hash);
  return m ? { section: m[1] && (MERGED[m[1]] ?? m[1]) } : null;
}

/** Settings, at `section`: the app's, where there is one, at the section
 * it's now (`machine`, for a machine's own: the one shown); Phones and
 * browsers, and every section on a daemon's own page, here. */
export function openSettings(section?: string, machine?: string) {
  const to = platformOrigin() ? settingsPath(section, machine) : null;
  if (to) return openPlatform(to);
  const hash = section ? `settings=${section}` : "settings";
  if (location.hash !== `#${hash}`) location.hash = hash;
}

export function closeSettings() {
  history.replaceState(null, "", location.pathname + location.search);
  window.dispatchEvent(new HashChangeEvent("hashchange"));
}
