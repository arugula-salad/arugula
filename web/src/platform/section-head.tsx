// A section's heading on a page (UI audit #162): a plain heading, what
// the section does (a small button), and a small "All ›" to its whole
// page on the right. The project home had each heading as an underlined
// link, with its "+" inside the heading.

import type { ComponentChildren } from "preact";
import { follow } from "./router";

export function SectionHead({ title, all, action, data }: { title: string; all?: string; action?: ComponentChildren; data?: string }) {
  const hook: Record<string, string> = data ? { [`data-${data}`]: "" } : {};
  return (
    <div class="sw-section-head">
      <h2>{title}</h2>
      {action}
      {all && (
        <a class="sw-all" href={all} aria-label={`All ${title.toLowerCase()}`} onClick={(e) => follow(e, all)} {...hook}>
          All ›
        </a>
      )}
    </div>
  );
}
