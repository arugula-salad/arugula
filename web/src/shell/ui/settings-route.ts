// The Settings page's route (Arugula #695): `#settings`, or `#settings=<id>`
// at one section. In its own file so a menu can open it without the page.

/** Sections that became one (Arugula #1108): an old link lands on it. */
const MERGED: Record<string, string> = { device: "push", "all-devices": "push" };

export function settingsRoute(): { section?: string } | null {
  const m = /^#settings(?:=([\w-]+))?$/.exec(location.hash);
  return m ? { section: m[1] && (MERGED[m[1]] ?? m[1]) } : null;
}

export function openSettings(section?: string) {
  const hash = section ? `settings=${section}` : "settings";
  if (location.hash !== `#${hash}`) location.hash = hash;
}

export function closeSettings() {
  history.replaceState(null, "", location.pathname + location.search);
  window.dispatchEvent(new HashChangeEvent("hashchange"));
}
