// The characters a person picks from (S44), by the number a rig frame
// carries. Drafted by Codex to ./character's contract, tuned by hand.

import type { Character } from "../character.ts";
import { bit } from "./bit.ts";
import { sprout } from "./sprout.ts";
import { wisp } from "./wisp.ts";

export const CHARACTERS: Character[] = [sprout, bit, wisp];

/** The character numbered `id`, or the first for one this page doesn't
 * know (a newer page's). */
export function characterOf(id: number): Character {
  return CHARACTERS.find((c) => c.id === id) ?? CHARACTERS[0];
}
