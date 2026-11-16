// After a split (Arugula #972): a note under the bar in both projects, for
// a week, until it's undone. Where the work left it counts what moved and
// where to ("25 issues, 1 board and 2 Docs moved to Rhythm."); where it
// arrived, where from ("Moved here from Personal."). Undo while the platform
// says the reader may. Only the latest split shows. Its × closes it for this
// reader, in this browser (Arugula #1039); the week's moves and their Undo
// stay in the project's Settings › General (../settings/general.tsx). Once
// undone it says where the work went back to, and a refused undo says why,
// in its place, until the page is left.

import { useEffect, useRef, useState } from "preact/hooks";
import { day } from "../../shared/time";
import { CloseIcon, IconButton } from "../../shared/ui/icons";
import { names } from "../links";
import { idOf } from "../paths";
import { go, pathOf, useRoute } from "../router";
import { session } from "../session";
import { barProject } from "../where";
import { splits, undo, type Split } from "./plan";
import "./move.css";

/** How long a split's note shows. */
const WEEK_MS = 7 * 24 * 60 * 60 * 1000;

/** The splits of the last week, latest first, not undone. */
export const recent = (l: Split[]) => l.filter((x) => !x.undone_at && Date.now() - x.moved_at < WEEK_MS);

/** Where a closed note is remembered, per split. */
const closedKey = (id: string) => `arugula.split-note-closed.${id}`;
// Where storage is refused, this page still remembers.
const closedHere = new Set<string>();

function closed(id: string): boolean {
  if (closedHere.has(id)) return true;
  try {
    return localStorage.getItem(closedKey(id)) !== null;
  } catch {
    return false;
  }
}

function close(id: string) {
  closedHere.add(id);
  try {
    localStorage.setItem(closedKey(id), "1");
  } catch {
    // without storage, it stays closed as long as the page
  }
}

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

/** Each shown list of splits: an undo anywhere reads them all again (the
 * note and Settings' list show on the same page). */
const readers = new Set<() => void>();

/** Undo `s`, as `project` sees it: what to say after, and, when `project`
 * was made for the split and is gone, the page to go to instead. A refusal
 * (a 409's too) throws, its words a sentence. */
export async function undoSplit(project: string, s: Split): Promise<{ text: string; gone?: string }> {
  try {
    // The undo is the project the work left's.
    const done = await undo(s.direction === "out" ? project : s.other.id, s.id);
    await session.load();
    const text = `Moved back to ${done.to.name}.`;
    readers.forEach((f) => f());
    return done.deleted && s.direction === "in" ? { text, gone: pathOf({ page: "project", project: done.to.id }, names) } : { text };
  } catch (e) {
    throw new Error(sentence((e as Error).message));
  }
}

/** A project's splits of the last week, read again when `project` changes;
 * `read` reads them again. Null until the first answer; none when it fails. */
export function useSplits(project: string | undefined) {
  const [list, setList] = useState<{ project: string; splits: Split[] } | null>(null);
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
    if (!project) return;
    void read(project);
    const again = () => void read(project);
    readers.add(again);
    return () => void readers.delete(again);
  }, [project]);
  return { splits: list && list.project === project ? list.splits : null, read };
}

export function SplitNote() {
  const r = useRoute();
  const project = barProject(r);
  const at = placeOf(location.pathname) + location.search;
  const { splits: list, read } = useSplits(project);
  const [busy, setBusy] = useState(false);
  const [, tick] = useState(0);
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
  const s = list ? recent(list)[0] : undefined;
  if (!s || closed(s.id)) return null;

  const undoIt = async () => {
    setBusy(true);
    let left = false;
    try {
      const { text, gone } = await undoSplit(project, s);
      if (gone) {
        // This project was made for the split, and is gone: to where the work is.
        kept = { at: placeOf(gone), text, refused: false };
        left = true;
        go(gone, true);
      } else {
        kept = { at, text, refused: false };
      }
    } catch (e) {
      kept = { at, text: (e as Error).message, refused: true };
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
      <IconButton
        label="Close"
        class="split-note-close"
        data-split-close
        onClick={() => {
          close(s.id);
          tick((n) => n + 1);
        }}
      >
        <CloseIcon size="sm" />
      </IconButton>
    </div>
  );
}

/** The project's moves of the last week, each with its Undo while the reader
 * may (Arugula #1039), for its Settings › General: where Undo stays once the
 * note is closed. Nothing when there are none. */
export function RecentSplits({ project }: { project: string }) {
  const { splits: list, read } = useSplits(project);
  const [busy, setBusy] = useState(false);
  const [said, setSaid] = useState<{ text: string; refused: boolean } | null>(null);
  const shown = list ? recent(list) : [];
  if (!shown.length && !said) return null;
  const undoIt = async (s: Split) => {
    setBusy(true);
    setSaid(null);
    try {
      const { text, gone } = await undoSplit(project, s);
      if (gone) return go(gone, true);
      setSaid({ text, refused: false });
    } catch (e) {
      setSaid({ text: (e as Error).message, refused: true });
    } finally {
      setBusy(false);
    }
    void read(project);
  };
  return (
    <>
      {shown.length > 0 && (
        <ul class="platform-list settings-rows" aria-label="Moves this week" data-recent-splits>
          {shown.map((s) => (
            <li key={s.id} data-recent-split={s.id}>
              <span>
                {s.direction === "out" ? `${movedWords(s.moved)} moved to ${s.other.name}` : `Moved here from ${s.other.name}`} · {day(s.moved_at)}
              </span>
              {s.may_undo && (
                <button type="button" class="link" disabled={busy} data-recent-split-undo onClick={() => void undoIt(s)}>
                  Undo
                </button>
              )}
            </li>
          ))}
        </ul>
      )}
      {said && (
        <p class={said.refused ? "control-error" : "dim"} role="status" data-recent-split-said={said.refused ? "refused" : "undone"}>
          {said.text}
        </p>
      )}
    </>
  );
}
