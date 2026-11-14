// Who's signed in on this page, for the shell's blocks and menus (Arugula
// #989): a board, team chat, a native issue, the pane's card chip, what's
// new. Control's page sets its `ControlSession`; the app sets one made from
// the platform's session (app/signed-in.ts), so the same blocks read as the
// person inside the app too. Without one, nobody is signed in here (a
// daemon's own page). What only control's page can do (passkeys, renaming
// a machine) reads its `ControlSession` (ui/people.tsx's
// `getControlSession`), which the app doesn't have, so it's hidden there.

import type { Phase, Team } from "./control";

export interface SignedIn {
  /** `ready` once someone is signed in. */
  phase: Phase;
  account: string;
  login: string;
  /** What other people see; may be empty. */
  name: string;
  /** Signed team rosters: control's page's; none in the app. */
  teams: Team[];
  subscribe(fn: () => void): () => void;
}

let signedIn: SignedIn | null = null;
export const getSignedIn = () => signedIn;
export function setSignedIn(s: SignedIn | null) {
  signedIn = s;
}
