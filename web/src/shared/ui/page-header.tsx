// A page's header (UI audit #157): the way back to where it came from, its
// title, what it's in (a state tag), what it does (actions), a line under
// it, and its counts. Every platform page draws it, and the shell's issue
// block (shared/issues/view.tsx), the Docs list and a board, so there's one
// of each: one back link, one title, one stat row. The layout and the type
// are ./page-header.css, wherever it's drawn (#686).
//
// A rule ends the header. It holds at most two labelled `actions`; a page
// with more puts the rest in `more` (⋯). Copy link, share and ⋯ are
// props of their own (`link`, `share`, `more`), always last and in that
// order, so they sit in the same place on every page.
//
// The title rule: a page's own name (Issues, Runs, Settings) is set in
// the display face in capitals; a name someone wrote (a project's, an
// issue's or a run's title, a workspace's) keeps its case (`named`), as the
// swarm's cards keep theirs.

import type { ComponentChildren } from "preact";
import { openMenu, type MenuItem } from "./menu";
import "./page-header.css";

export interface BackLink {
  href: string;
  /** Where it goes, by that page's name: "Issues". */
  label: string;
  /** Following it without a reload (the platform page's `follow`). */
  onClick?: (e: MouseEvent) => void;
  /** Its data-* hook, for tests. */
  data?: string;
}

const isBackLink = (b: unknown): b is BackLink => typeof b === "object" && b !== null && "href" in b && "label" in b && !("type" in b);

export function PageHeader({
  title,
  named,
  back,
  state,
  actions,
  link,
  share,
  more,
  sub,
  stats,
  children,
  class: cls,
}: {
  /** null: no heading (a form in its place, editing it). */
  title: ComponentChildren | null;
  /** The title is someone's words: it keeps its case. */
  named?: boolean;
  /** The way back: one link, or the page's own line of them (a board's, with its Workers). */
  back?: BackLink | ComponentChildren;
  /** Beside the title: its state, as a tag. */
  state?: ComponentChildren;
  /** At the title's right: the page's own buttons, two labelled ones at most. */
  actions?: ComponentChildren;
  /** Copy link (a `CopyLinkButton`), after the actions. */
  link?: ComponentChildren;
  /** Share, after copy link. */
  share?: ComponentChildren;
  /** ⋯, last: what doesn't fit in `actions`, and the page's rarer items. */
  more?: MenuItem[];
  /** Under the title: a line about it. */
  sub?: ComponentChildren;
  /** Its counts (a `.sw-stats` row). */
  stats?: ComponentChildren;
  /** Anything else in the header, last (a title being edited). */
  children?: ComponentChildren;
  class?: string;
}) {
  const link1 = isBackLink(back) ? back : undefined;
  const hook: Record<string, string> = link1?.data ? { [`data-${link1.data}`]: "" } : {};
  const tools = !!(link || share || (more && more.length > 0));
  return (
    <header class={cls ? `page-head ${cls}` : "page-head"} data-page-head>
      {link1 ? (
        <a class="page-back" href={link1.href} onClick={link1.onClick} {...hook}>
          <span aria-hidden="true">‹ </span>
          {link1.label}
        </a>
      ) : (
        back && <div class="page-back page-back-line">{back}</div>
      )}
      {(title !== null || state || actions || tools) && (
        <div class="page-title">
          {title !== null && <h1 class={named ? "named" : undefined}>{title}</h1>}
          {state}
          {(actions || tools) && (
            <div class="page-actions">
              {actions}
              {tools && (
                <span class="page-tools" data-page-tools>
                  {link}
                  {share}
                  {more && more.length > 0 && (
                    <button type="button" class="page-more" aria-label="More" aria-haspopup="menu" data-page-more onClick={(e) => openMenu(e, more, e.currentTarget)}>
                      ⋯
                    </button>
                  )}
                </span>
              )}
            </div>
          )}
        </div>
      )}
      {children}
      {sub && <div class="page-sub">{sub}</div>}
      {stats}
    </header>
  );
}
