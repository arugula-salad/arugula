// A pane's card chip (Arugula inversion, on a phone): what it says, cut to
// the room it has, the picker's choices and what an answer picks. Plain
// functions, for the chip and its tests (scripts/pane-card.test.mjs).
// The picker's choices and answers are shared/card-pick.ts's (the Sessions
// page asks with them too), kept here for the chip and its tests.
export { cardChoices, cardNumber, cardPicked } from "../../shared/card-pick.ts";

/** What the chip says: the project (left out when there's no room), the
 * card (`#983`, null for none) and its title (null when cut away). */
export interface CardChipText {
  project: string | null;
  card: string | null;
  title: string | null;
}

/** The fewest characters of a title worth showing; under it, the title goes. */
const TITLE_MIN = 8;
/** What the separators take: ` › ` and the space before the title. */
const SEP = 3;
const GAP = 1;

/** The chip in `room` characters: `Arugula › #983 Title`, the title cut
 * first, then only `#983`; null, no chip, when even `#983` doesn't fit.
 * Without a card, `Arugula › Put on a card`, then only the words, then
 * nothing. */
export function cardChipText(project: string, card: string | null, title: string | null, room: number): CardChipText | null {
  if (!card) {
    if (room < PUT_ON.length) return null;
    return { project: project.length + SEP + PUT_ON.length <= room ? project : null, card: null, title: null };
  }
  if (room < card.length) return null;
  const head = project.length + SEP + card.length;
  const t = title?.trim() ?? "";
  if (!t) return head <= room ? { project, card, title: null } : { project: null, card, title: null };
  const left = room - head - GAP;
  if (t.length <= left) return { project, card, title: t };
  if (left >= TITLE_MIN) return { project, card, title: `${t.slice(0, left - 1).trimEnd()}…` };
  return { project: null, card, title: null };
}

export const PUT_ON = "Put on a card";
export const CHANGE = "Change card";
export const TAKE_OFF = "Take off the card";

/** The board page's address of a card's row (its drawer opens there), else
 * its issue's page, else the project's issues. */
export function cardPath(project: string, at: { board: string; row: string } | null, issue: string | null): string {
  const p = encodeURIComponent(project);
  if (at) return `/p/${p}/boards/${encodeURIComponent(at.board)}#row=${encodeURIComponent(at.row)}`;
  return issue ? `/p/${p}/issues/${encodeURIComponent(issue)}` : `/p/${p}/issues`;
}
