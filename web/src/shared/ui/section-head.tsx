// A section's heading on a page (UI audit #162): a plain heading in the
// display face, what the section does (a small button), and a small "All ›"
// to its whole page on the right. The project home had each heading as an
// underlined link, with its "+" inside the heading. Boards' groups draw it
// too (#686), with their count and a rule under it. It lives in shared/ so
// the shell's board block can; `onAll` is how the page follows the link
// without a reload (the platform's `follow`).

import type { ComponentChildren } from "preact";
import "./section-head.css";

export function SectionHead({
  title,
  count,
  all,
  onAll,
  action,
  data,
  titleData,
  rule,
}: {
  title: ComponentChildren;
  /** How many it holds: a mono number after the heading. */
  count?: ComponentChildren;
  /** Where "All ›" goes. */
  all?: string;
  onAll?: (e: MouseEvent) => void;
  action?: ComponentChildren;
  /** The "All ›" link's data-* hook, for tests. */
  data?: string;
  /** The heading's data-* hook. */
  titleData?: string;
  /** A 1px rule under the head. */
  rule?: boolean;
}) {
  const hook: Record<string, string> = data ? { [`data-${data}`]: "" } : {};
  const own: Record<string, string> = titleData ? { [`data-${titleData}`]: "" } : {};
  const name = typeof title === "string" ? title.toLowerCase() : "";
  return (
    <div class={rule ? "sw-section-head section-head ruled" : "sw-section-head section-head"}>
      <h2 {...own}>{title}</h2>
      {count !== undefined && count !== false && <span class="section-count">{count}</span>}
      {action}
      {all && (
        <a class="sw-all" href={all} aria-label={`All ${name}`} onClick={onAll} {...hook}>
          All ›
        </a>
      )}
    </div>
  );
}
