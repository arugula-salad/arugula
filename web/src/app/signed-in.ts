// Who's signed in, for the shell's blocks inside the app (Arugula #989):
// the platform's session (`session.me`) as the shell's `SignedIn`, which
// control's page gets from its `ControlSession`. Without it a board, a
// native issue or team chat in a machine's pane said you weren't signed in.
// Teams are control's signed rosters, which the app doesn't hold (as
// AppSession's), so it has none.

import type { PlatformMe } from "../shared/proto";
import type { Phase } from "../shell/control";
import type { SignedIn } from "../shell/signed-in";

/** What it reads of the platform's session (platform/session.ts). */
export interface PlatformSession {
  me: PlatformMe | null;
  state: null | "ready" | "signed-out" | { error: string };
  subscribe(fn: () => void): () => void;
}

/** The platform's session as the shell's `SignedIn`: ready once the
 * platform has said who's reading. */
export function signedInOf(s: PlatformSession): SignedIn {
  const phase = (): Phase => (s.state === "ready" && s.me ? "ready" : s.state === null ? "loading" : s.state === "signed-out" ? "signed-out" : "error");
  return {
    get phase() {
      return phase();
    },
    get account() {
      return s.me?.account ?? "";
    },
    get login() {
      return s.me?.login ?? "";
    },
    get name() {
      return s.me?.name ?? "";
    },
    teams: [],
    subscribe: (fn) => s.subscribe(fn),
  };
}
