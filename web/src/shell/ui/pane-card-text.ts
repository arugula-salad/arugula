// A pane's card chip (Arugula inversion): what it says, cut to the room a
// pane's bar has, the picker's choices and what an answer picks. Plain
// functions, for the chip and its tests (scripts/pane-card.test.mjs).
import type { NativeIssue } from "../../shared/proto";
import type { PromptChoice } from "../../shared/ui/menu";

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

/** What a pane's bar takes besides its title and the chip, in px: its
 * padding, number, glyph, status, Focus and ⋯, and the gaps between. */
const BAR_FIXED = 200;
/** What the chip takes besides its text: its padding and border, the ▾
 * beside it and the gaps inside. */
const CHIP_FIXED = 50;
/** The title's share of a bar with a card chip (pane-chrome.css caps it
 * there), and its least (6ch). */
export const TITLE_SHARE = 0.4;
const TITLE_LEAST = 45;
/** About a character's width: 11px text in the chip, 13px in the title. */
const CHIP_CH = 6.5;
const TITLE_CH = 7.5;

/** About how many characters of chip text fit in a pane's bar `width` px
 * wide, beside its title (`title` characters) and the rest of the bar. */
export function chipRoom(width: number, title: number): number {
  const said = Math.min(Math.max(title * TITLE_CH, TITLE_LEAST), width * TITLE_SHARE);
  return Math.max(0, Math.floor((width - BAR_FIXED - said - CHIP_FIXED) / CHIP_CH));
}

/** A card's number in its reference: `#983` → 983, else null (an `iss_`). */
export function cardNumber(card: string): number | null {
  const m = /^#?(\d+)$/.exec(card.trim());
  return m ? Number(m[1]) : null;
}

/** The project's open issues as the picker's choices, newest first, the
 * pane's own card marked and first. */
export function cardChoices(open: Pick<NativeIssue, "number" | "title">[], current: string | null): PromptChoice[] {
  const now = current ? cardNumber(current) : null;
  const choices = open.map((i) => ({ value: `#${i.number}`, label: `#${i.number} ${i.title}`, strong: i.number === now }));
  choices.sort((a, b) => Number(b.strong) - Number(a.strong));
  return choices;
}

/** The card an answer names: one chosen, a number typed (`983`, `#983`),
 * an `iss_` id, or the one open issue whose title has the words. Null when
 * it names none, or more than one. */
export function cardPicked(answer: string, open: Pick<NativeIssue, "id" | "number" | "title">[]): string | null {
  const a = answer.trim();
  if (!a) return null;
  const n = cardNumber(a);
  if (n !== null) return `#${n}`;
  if (/^iss_[0-9a-f]+$/i.test(a)) return a.toLowerCase();
  const hits = open.filter((i) => i.title.toLowerCase().includes(a.toLowerCase()));
  return hits.length === 1 ? `#${hits[0].number}` : null;
}

/** The board page's address of a card's row (its drawer opens there), else
 * its issue's page, else the project's issues. */
export function cardPath(project: string, at: { board: string; row: string } | null, issue: string | null): string {
  const p = encodeURIComponent(project);
  if (at) return `/p/${p}/boards/${encodeURIComponent(at.board)}#row=${encodeURIComponent(at.row)}`;
  return issue ? `/p/${p}/issues/${encodeURIComponent(issue)}` : `/p/${p}/issues`;
}
