// The hints (./index, DECISIONS.md "Hints come from facts, once per
// account"), in the order they're offered when two facts hold at once. A
// new one is an entry here: its id (never reused), the fact, one line of
// copy, its action and its place.

import type { Hint } from "./index";
import { listeningPort } from "./ports";

export const HINTS: Hint[] = [
  {
    // A dev server started in a pane: it can be beside the terminal.
    id: "port-listening",
    place: "chip",
    fact: listeningPort,
    copy: (f) => `:${f.args.port} is listening`,
    action: { label: "Open beside", command: "port.open-beside" },
  },
];
