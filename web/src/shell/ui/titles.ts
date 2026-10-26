// Control's browser-tab titles (UI audit #170): what's shown, then the
// machine, then Arugula, as the platform page's are (platform/titles.ts:
// "Runs · Acme · Arugula"), so tabs and history read apart: "api · geek ·
// Arugula", "Swarm · Arugula". A name not yet known is left out. Tested in
// scripts/platform-titles.test.mjs; app.tsx keeps the tab's title to it.

import type { Place } from "./places";

const APP = "Arugula";

/** The title for `place` on `machine`, in `session` (the panes'). */
export function shellTitle(place: Place, machine: string | null | undefined, session: string | null | undefined): string {
  const parts = (...xs: (string | null | undefined)[]) => [...xs, APP].filter(Boolean).join(" · ");
  switch (place) {
    // Every machine's: no one machine's name.
    case "swarm":
      return parts("Swarm");
    case "agents":
      return parts("Agents", machine);
    case "docs":
      return parts("Docs", machine);
    case "overview":
      return parts("Sessions", machine);
    case "panes":
      return parts(session, machine);
  }
}
