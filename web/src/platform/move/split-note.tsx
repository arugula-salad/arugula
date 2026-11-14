// After a split (Arugula #972): a note under the bar in both projects, for
// a week, until it's undone. Where the work left it counts what moved and
// where to ("25 issues, 1 board and 2 Docs moved to Rhythm."); where it
// arrived, where from ("Moved here from Personal."). Undo while the platform
// says the reader may. Only the latest split shows, and the note has no
// dismiss. Once undone it says where the work went back to, and a refused
// undo says why, in its place, until the page is left.

import { useEffect, useRef, useState } from "preact/hooks";
import { names } from "../links";
import { idOf } from "../paths";
import { go, pathOf, useRoute } from "../router";
import { session } from "../session";
import { barProject } from "../where";
import { splits, undo, type Split } from "./plan";
import "./move.css";

/** How long a split's note shows. */
const WEEK_MS = 7 * 24 * 60 * 60 * 1000;

/** What the note counts, in its order. */
const WORDS: [keyof Split["moved"], string][] = [
  ["repos", "repo"],
  ["issues", "issue"],
  ["boards", "board"],
  ["docs", "Doc"],
  ["workspaces", "cloud workspace"],
  ["environments", "environment"],
  ["runs", "run"],
  ["sessions", "session"],
  ["drafts", "draft"],
];

/** "25 issues, 1 board and 2 Docs", the non-zero counts; "Work" with none. */
export function movedWords(m: Split["moved"]): string {
  const bits = WORDS.filter(([k]) => m[k] > 0).map(([k, w]) => `${m[k]} ${w}${m[k] === 1 ? "" : "s"}`);
  if (!bits.length) return "Work";
  return bits.length === 1 ? bits[0] : `${bits.slice(0, -1).join(", ")} and ${bits.at(-1)}`;
}

/** The note's words for split `s`, without its Undo. */
export const splitWords = (s: Split) => (s.direction === "out" ? `${movedWords(s.moved)} moved to ${s.other.name}.` : `Moved here from ${s.other.name}.`);

/** The page, without slugs: the note's "until the page is left". */
const placeOf = (path: string) =>
  path
    .split("/")
    .map((x) => idOf(x) ?? x)
    .join("/");

/** What replaces the note until the page at `at` is left: the undo's
 * outcome, or the platform's refusal. */
let kept: { at: string; text: string; refused: boolean } | null = null;

const sentence = (s: string) => (s ? `${s[0].toUpperCase()}${s.slice(1)}${/[.!?]$/.test(s) ? "" : "."}` : s);

export function SplitNote() {
  const r = useRoute();
  const project = barProject(r);
  const at = placeOf(location.pathname) + location.search;
  const [list, setList] = useState<{ project: string; splits: Split[] } | null>(null);
  const [busy, setBusy] = useState(false);
  const [, tick] = useState(0);
  // Only the latest read lands: an older one may be another project's.
  const asked = useRef(0);
  const read = (p: string) => {
    const n = ++asked.current;
    return splits(p).then(
      (l) => n === asked.current && setList({ project: p, splits: l }),
      () => n === asked.current && setList({ project: p, splits: [] }),
    );
  };
  useEffect(() => {
    if (project) void read(project);
  }, [project]);
  useEffect(() => {
    if (kept && kept.at !== at) kept = null;
  }, [at]);

  if (!project) return null;
  if (kept?.at === at) {
    return (
      <div class={`ws-stale ${kept.refused ? "" : "maintenance-note "}split-note`} role="status" data-split-note={kept.refused ? "refused" : "undone"}>
        {kept.text}
      </div>
    );
  }
  const s = list?.project === project ? list.splits.find((x) => !x.undone_at && Date.now() - x.moved_at < WEEK_MS) : undefined;
  if (!s) return null;

  const undoIt = async () => {
    setBusy(true);
    let left = false;
    try {
      // The undo is the project the work left's.
      const done = await undo(s.direction === "out" ? project : s.other.id, s.id);
      const text = `Moved back to ${done.to.name}.`;
      await session.load();
      if (done.deleted && s.direction === "in") {
        // This project was made for the split, and is gone: to where the work is.
        const to = pathOf({ page: "project", project: done.to.id }, names);
        kept = { at: placeOf(to), text, refused: false };
        left = true;
        go(to, true);
      } else {
        kept = { at, text, refused: false };
      }
    } catch (e) {
      kept = { at, text: sentence((e as Error).message), refused: true };
    } finally {
      setBusy(false);
      tick((n) => n + 1);
      if (!left) void read(project);
    }
  };

  return (
    <div class="ws-stale maintenance-note split-note" role="status" data-split-note={s.direction}>
      <span>{splitWords(s)}</span>
      {s.may_undo && (
        <button type="button" class="link" disabled={busy} data-split-undo onClick={() => void undoIt()}>
          Undo
        </button>
      )}
    </div>
  );
}
