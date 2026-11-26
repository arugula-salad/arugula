// Panel (kit §4.1, 9): the agent panel on the right. Its tabs, a head (who,
// where, what it's doing), the transcript or a well, and the composer.

import type { ComponentChildren } from "preact";
import { Surface } from "./surface";
import "./kit.css";

export function Panel({ label, tabs, head, composer, children }: { label: string; tabs?: ComponentChildren; head: ComponentChildren; composer?: ComponentChildren; children: ComponentChildren }) {
  return (
    <Surface kind="section" as="aside" class="k-panel" aria-label={label} data-kit="panel">
      {tabs}
      <div class="k-panel-head">{head}</div>
      <div class="k-panel-body">{children}</div>
      {composer}
    </Surface>
  );
}
