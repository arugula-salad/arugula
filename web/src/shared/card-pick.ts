// Picking a card for a session (Arugula inversion): the project's open
// issues as a picker's choices, and the card an answer names. The pane
// chip (shell/ui/pane-card.tsx) and the Sessions page both ask with them.
// Plain functions, for their tests (scripts/pane-card.test.mjs).
import type { NativeIssue } from "./proto";
import type { PromptChoice } from "./ui/menu";

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
