// The characters a person picks from (S44), by the number a rig frame
// carries. Drafted by Codex to ./character's contract, tuned by hand.

import type { Character } from "../character";
import { bit } from "./bit";
import { sprout } from "./sprout";
import { wisp } from "./wisp";

export const CHARACTERS: Character[] = [sprout, bit, wisp];

/** The character numbered `id`, or the first for one this page doesn't
 * know (a newer page's). */
export function characterOf(id: number): Character {
  return CHARACTERS.find((c) => c.id === id) ?? CHARACTERS[0];
}
