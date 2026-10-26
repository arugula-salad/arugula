// A page's header (UI audit #157): the way back to where it came from, its
// title, what it's in (a state tag), what it does (actions), a line under
// it, and its counts. Every platform page draws it, and the shell's issue
// block (shared/issues/view.tsx), so there's one of each: one back link,
// one title, one stat row. The layout is ./page-header.css; the swarm's
// type on the platform page is platform/swarm.css.
//
// The title rule: a page's own name (Issues, Runs, Settings) is set in
// the display face in capitals; a name someone wrote (a project's, an
// issue's or a run's title, a workspace's) keeps its case (`named`), as the
// swarm's cards keep theirs.

import type { ComponentChildren } from "preact";
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

export function PageHeader({
  title,
  named,
  back,
  state,
  actions,
  sub,
  stats,
  children,
  class: cls,
}: {
  /** null: no heading (a form in its place, editing it). */
  title: ComponentChildren | null;
  /** The title is someone's words: it keeps its case. */
  named?: boolean;
  back?: BackLink;
  /** Beside the title: its state, as a tag. */
  state?: ComponentChildren;
  /** At the title's right: the page's own buttons. */
  actions?: ComponentChildren;
  /** Under the title: a line about it. */
  sub?: ComponentChildren;
  /** Its counts (a `.sw-stats` row). */
  stats?: ComponentChildren;
  /** Anything else in the header, last (a title being edited). */
  children?: ComponentChildren;
  class?: string;
}) {
  const hook: Record<string, string> = back?.data ? { [`data-${back.data}`]: "" } : {};
  return (
    <header class={cls ? `page-head ${cls}` : "page-head"} data-page-head>
      {back && (
        <a class="page-back" href={back.href} onClick={back.onClick} {...hook}>
          <span aria-hidden="true">‹ </span>
          {back.label}
        </a>
      )}
      {(title !== null || state || actions) && (
        <div class="page-title">
          {title !== null && <h1 class={named ? "named" : undefined}>{title}</h1>}
          {state}
          {actions && <div class="page-actions">{actions}</div>}
        </div>
      )}
      {children}
      {sub && <div class="page-sub">{sub}</div>}
      {stats}
    </header>
  );
}
