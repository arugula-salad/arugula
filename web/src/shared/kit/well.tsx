// Well (kit §4.1, 4): inset, flat and mono, with its label set into the
// frame. Only for a terminal, an agent pane, or something to copy (a
// to-do's how). The frame brightens while focus is inside.

import type { ComponentChildren } from "preact";
import "./kit.css";

export function Well({ label, children }: { label?: string; children: ComponentChildren }) {
  return (
    <div class="k-well" data-kit="well" role={label ? "group" : undefined} aria-label={label}>
      {label ? (
        <span class="k-well-label" aria-hidden="true">
          {label}
        </span>
      ) : null}
      {children}
    </div>
  );
}
