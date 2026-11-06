// What every block draws when it has nothing else to (UI audit #178): one
// error card, one empty or loading line, and one way to open what it shows
// outside the page. Before, each block had its own: a body-coloured
// headline with the raw error repeating it, "…", "Loading…", "Starting…",
// centred in some and top-left in others, and four kinds of "open ↗".

import type { ComponentChildren } from "preact";
import { useEffect, useState } from "preact/hooks";
import "./block-states.css";

/** How long a load must take before it says so: a fast one never flashes. */
export const LOADING_DELAY_MS = 300;

export function useAfter(ms: number): boolean {
  const [late, setLate] = useState(ms <= 0);
  useEffect(() => {
    const t = setTimeout(() => setLate(true), ms);
    return () => clearTimeout(t);
  }, [ms]);
  return late;
}

/** The detail without the headline it repeats: "can't read the issue: 404"
 * under "Can't read the issue" says just "404". */
export function detailOf(title: string, detail: string | null | undefined): string | null {
  const d = (detail ?? "").trim();
  if (!d) return null;
  const t = title.trim().replace(/[.…:]+$/, "").toLowerCase();
  if (d.toLowerCase() === t) return null;
  if (d.toLowerCase().startsWith(t)) {
    const rest = d.slice(t.length).replace(/^[\s:,.-]+/, "");
    return rest || null;
  }
  // "nothing is answering on port 18999: refused" under "Nothing is
  // answering on :18999": the clause before the colon says it again.
  const colon = d.indexOf(": ");
  const lead = (x: string) => x.toLowerCase().split(/\s+/).slice(0, 3).join(" ");
  if (colon > 0 && lead(d.slice(0, colon)) === lead(t)) return d.slice(colon + 2) || null;
  return d;
}

/**
 * A block's error: a headline ("Can't …"), the detail dim under it (without what
 * the headline already says), a danger accent, and Try again when `retry`
 * is given. `children` go under it (another login to use, a hint).
 * `inline` sits in a column with the block's other content; otherwise it
 * fills the block, centred.
 */
export function BlockError({
  title,
  detail,
  retry,
  retryLabel = "Try again",
  inline,
  children,
  ...data
}: {
  title: string;
  detail?: string | null;
  retry?: () => void;
  retryLabel?: string;
  inline?: boolean;
  children?: ComponentChildren;
  [attr: `data-${string}`]: unknown;
}) {
  const more = detailOf(title, detail);
  return (
    <div class={inline ? "block-error inline" : "block-error"} role="alert" {...data}>
      <div class="block-error-card">
        <p class="block-error-title">{title}</p>
        {more && <p class="block-error-detail">{more}</p>}
        {children}
        {retry && (
          <div class="block-error-actions">
            <button class="secondary small" onClick={retry}>
              {retryLabel}
            </button>
          </div>
        )}
      </div>
    </div>
  );
}

/**
 * An empty place: a statement with a full stop for a title, what the thing
 * is and where it comes from in a line or two, and at most one action (a
 * `.primary` the caller draws; none when the action is already on screen or
 * the reader can't take it). `where="page"` is a card in a page's column;
 * `"pane"` fills a block and centres the card. `children` go under the why.
 */
export function EmptyState({
  title,
  why,
  action,
  where = "pane",
  children,
  ...data
}: {
  title: string;
  why?: ComponentChildren;
  action?: ComponentChildren;
  where?: "page" | "pane";
  children?: ComponentChildren;
  [attr: `data-${string}`]: unknown;
}) {
  return (
    <div class={`empty-state in-${where}`} {...data}>
      <div class="empty-state-card">
        <h3 class="empty-state-title">{title}</h3>
        {why && <p class="empty-state-why">{why}</p>}
        {children}
        {action && <div class="empty-state-action">{action}</div>}
      </div>
    </div>
  );
}

/** No match, or "None yet." in a settings section: a dim line, not a card,
 * with Clear when something narrows the list. */
export function EmptyLine({
  children,
  clear,
  clearLabel = "Clear",
  role,
  ...data
}: {
  children: ComponentChildren;
  clear?: () => void;
  clearLabel?: string;
  role?: "status";
  [attr: `data-${string}`]: unknown;
}) {
  return (
    <p class="empty-line" role={role} {...data}>
      {children}
      {clear && (
        <>
          {" "}
          <button type="button" class="link" onClick={clear}>
            {clearLabel}
          </button>
        </>
      )}
    </p>
  );
}

/** "Loading the pull request…": says nothing for the first
 * {@link LOADING_DELAY_MS}, so a quick load doesn't flash it. `inline` is a
 * dim line in a column; otherwise it fills the block, centred. */
export function BlockLoading({ what, inline, ...data }: { what: string; inline?: boolean; [attr: `data-${string}`]: unknown }) {
  const late = useAfter(LOADING_DELAY_MS);
  if (!late) return null;
  return (
    <div class={inline ? "empty-line" : "block-loading"} role="status" {...data}>
      Loading {what}…
    </div>
  );
}

/** What a block shows, opened outside the page: one ↗ button in its bar. */
export function OpenOutside({ href, ...data }: { href: string; [attr: `data-${string}`]: unknown }) {
  return (
    <a class="block-open" href={href} target="_blank" rel="noopener noreferrer" title="Open in a new tab" aria-label="Open in a new tab" {...data}>
      ↗
    </a>
  );
}
