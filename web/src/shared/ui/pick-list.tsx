// Picking several things out of many in a form (Arugula #971): a box per
// row, in groups, with a search and "Select all matching" over them, and a
// page of rows at a time, so it holds hundreds. A row that's in because of
// something else is ticked and locked, and its tag says why ("with
// owner/repo"); a row with no box says in its detail why it can't be
// picked here. And `CountList`: a count that opens into its rows, a page at
// a time, for "what comes along".
//
// The list page's own ticking (`ListPage selectable`) is for acting on rows
// you're browsing; this is a field in a form.

import type { ComponentChildren } from "preact";
import { useState } from "preact/hooks";
import { ChevronIcon } from "./icons";
import { Chip } from "./state-chip";
import "./pick-list.css";

/** How many rows show before "Show more". */
export const PAGE = 25;

export interface Pick {
  key: string;
  title: ComponentChildren;
  /** A dim line under the title. */
  detail?: ComponentChildren;
  on: boolean;
  /** Why it's in without its own pick ("with owner/repo"): ticked and locked. */
  why?: string | null;
  /** No box: it can't be picked here (`detail` says why). */
  fixed?: boolean;
}

export interface PickGroup {
  key: string;
  title?: string;
  /** A plain line under the group's title. */
  note?: ComponentChildren;
  /** Beside the note: a link to where it can be done. */
  action?: ComponentChildren;
  items: Pick[];
}

export function PickList({
  label,
  groups,
  toggle,
  search,
  ...data
}: {
  label: string;
  groups: PickGroup[];
  toggle: (key: string, on: boolean) => void;
  /** A search over the rows (the page filters `groups` by it), and ticking
   * every pickable row it leaves. */
  search?: { query: string; setQuery: (q: string) => void; placeholder: string; matching: number; all: () => void; clear: () => void; picked: number };
  [hook: `data-${string}`]: string | undefined;
}) {
  const rows = groups.reduce((n, g) => n + g.items.length, 0);
  return (
    <div class="pick-list" role="group" aria-label={label} {...data}>
      {search && (
        <div class="pick-search">
          <input class="field" type="search" aria-label={search.placeholder} placeholder={search.placeholder} value={search.query} data-pick-search onInput={(e) => search.setQuery(e.currentTarget.value)} />
          <div class="pick-search-acts">
            <button type="button" class="link" disabled={!search.matching} data-pick-all onClick={search.all}>
              {search.query.trim() ? `Select all matching (${search.matching})` : `Select all (${search.matching})`}
            </button>
            {search.picked > 0 && (
              <button type="button" class="link" data-pick-clear onClick={search.clear}>
                Clear {search.picked}
              </button>
            )}
          </div>
        </div>
      )}
      {groups.map((g) => (g.items.length ? <Group key={g.key} g={g} toggle={toggle} /> : null))}
      {!rows && <p class="pick-none dim">{search?.query.trim() ? "Nothing matches." : "None."}</p>}
    </div>
  );
}

function Group({ g, toggle }: { g: PickGroup; toggle: (key: string, on: boolean) => void }) {
  const [shown, setShown] = useState(PAGE);
  const more = g.items.length - shown;
  return (
    <div class="pick-group" data-pick-group={g.key}>
      {g.title && (
        <div class="pick-group-head">
          <span class="pick-group-title">{g.title}</span>
          <span class="pick-group-n">{g.items.length}</span>
        </div>
      )}
      {(g.note || g.action) && (
        <p class="pick-group-note">
          {g.note} {g.action}
        </p>
      )}
      <ul class="pick-rows">
        {g.items.slice(0, shown).map((it) => (
          <li key={it.key} class="pick-row" data-on={it.on || !!it.why} data-pick={it.key}>
            {it.fixed ? (
              <span class="pick-box-none" />
            ) : (
              <input type="checkbox" checked={it.on || !!it.why} disabled={!!it.why} aria-describedby={it.why ? `why-${it.key}` : undefined} onChange={(e) => toggle(it.key, e.currentTarget.checked)} id={`pick-${it.key}`} />
            )}
            <label class="pick-text" for={it.fixed ? undefined : `pick-${it.key}`}>
              <span class="pick-title">{it.title}</span>
              {it.why && (
                <Chip kind="tag" class="pick-why" data-why={it.key}>
                  <span id={`why-${it.key}`}>{it.why}</span>
                </Chip>
              )}
              {it.detail && <span class="pick-detail">{it.detail}</span>}
            </label>
          </li>
        ))}
      </ul>
      {more > 0 && (
        <button type="button" class="link pick-more" data-pick-more onClick={() => setShown(shown + PAGE)}>
          Show {Math.min(more, PAGE)} more{more > PAGE ? ` of ${more}` : ""}
        </button>
      )}
    </div>
  );
}

export interface Counted {
  key: string;
  title: ComponentChildren;
  /** At the right, dim: a number's change, a count. */
  meta?: ComponentChildren;
}

/** "25 issues ›": the count, opening into its rows a page at a time. With
 * no rows, a line that doesn't open; a count of none isn't drawn. */
export function CountList({ n, label, rows, note, ...data }: { n: number; label: string; rows?: Counted[]; note?: ComponentChildren; [hook: `data-${string}`]: string | undefined }) {
  const [open, setOpen] = useState(false);
  const [shown, setShown] = useState(PAGE);
  const more = (rows?.length ?? 0) - shown;
  if (!n) return null;
  const head = (
    <>
      <span class="count-list-n">{n}</span> {label}
    </>
  );
  return (
    <div class="count-list" {...data}>
      {rows?.length ? (
        <button type="button" class="count-list-head" aria-expanded={open} onClick={() => setOpen(!open)}>
          <ChevronIcon open={open} />
          {head}
        </button>
      ) : (
        <div class="count-list-head still">{head}</div>
      )}
      {note && <p class="count-list-note">{note}</p>}
      {open && rows && (
        <>
          <ul class="count-list-rows">
            {rows.slice(0, shown).map((r) => (
              <li key={r.key}>
                <span class="count-list-title">{r.title}</span>
                {r.meta && <span class="count-list-meta">{r.meta}</span>}
              </li>
            ))}
          </ul>
          {more > 0 && (
            <button type="button" class="link pick-more" onClick={() => setShown(shown + PAGE)}>
              Show {Math.min(more, PAGE)} more{more > PAGE ? ` of ${more}` : ""}
            </button>
          )}
        </>
      )}
    </div>
  );
}
