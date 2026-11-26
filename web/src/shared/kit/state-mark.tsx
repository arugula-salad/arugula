// StateMark (kit §4.1, 5): what a thing is doing, as a mark and always its
// word, so the state never rests on colour alone.

import "./kit.css";

export type State = "working" | "asks" | "waits" | "stuck" | "failing" | "done" | "idle";

/** Each state's mark and its word. Working breathes. */
export const MARKS: Record<State, { mark: string; word: string }> = {
  working: { mark: "◉", word: "working" },
  asks: { mark: "◆", word: "asks" },
  waits: { mark: "◇", word: "waits" },
  stuck: { mark: "▲", word: "stuck" },
  failing: { mark: "✕", word: "failing" },
  done: { mark: "✓", word: "done" },
  idle: { mark: "○", word: "idle" },
};

/** The mark alone, for a row that says its state in words elsewhere (a
 * NeedRow's "asks you"). */
export function Mark({ state }: { state: State }) {
  return (
    <span class="k-state" data-state={state} aria-hidden="true">
      <i class="k-mark">{MARKS[state].mark}</i>
    </span>
  );
}

/** The mark and its word; `word` says it differently ("asks you"). */
export function StateMark({ state, word }: { state: State; word?: string }) {
  return (
    <span class="k-state" data-state={state} data-kit="state">
      <i class="k-mark" aria-hidden="true">
        {MARKS[state].mark}
      </i>
      <span class="k-state-word m-meta">{word ?? MARKS[state].word}</span>
    </span>
  );
}
