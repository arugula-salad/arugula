// What every block draws when it has nothing else to (UI audit #178): one
// error card, one empty or loading line, and one way to open what it shows
// outside the page. Before, each block had its own: a body-coloured
// headline with the raw error repeating it, "…", "Loading…", "Starting…",
// centred in some and top-left in others, and four kinds of "open ↗".

import type { ComponentChildren } from "preact";

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
 * A block's error: a bold headline, the detail dim under it (without what
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

/** Nothing to show yet, or at all: one dim line (and what follows it),
 * centred in the block. */
export function BlockEmpty({ children, ...data }: { children: ComponentChildren; [attr: `data-${string}`]: unknown }) {
  return (
    <div class="block-empty" {...data}>
      {children}
    </div>
  );
}

/** "Loading the pull request…", centred, in the one place every block
 * says it. */
export function BlockLoading({ what, ...data }: { what: string; [attr: `data-${string}`]: unknown }) {
  return (
    <div class="block-empty" role="status" {...data}>
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
