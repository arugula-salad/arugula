// Crumb (kit §4.1, 6): where you are, as a path you can climb. The first
// step is the project; the last is here, and isn't a link.

import { Fragment } from "preact";
import { ChevronIcon } from "../ui/icons";
import "./kit.css";

export interface Step {
  label: string;
  /** Climb to it (the project's opens the project list). */
  go?: () => void;
  /** An id or a machine: set in the id face. */
  id?: boolean;
}

export function Crumb({ steps }: { steps: Step[] }) {
  return (
    <nav class="k-crumb" aria-label="Where you are" data-kit="crumb">
      {steps.map((s, i) => {
        const here = i === steps.length - 1;
        return (
          <Fragment key={i}>
            {i ? (
              <span class="k-crumb-sep" aria-hidden="true">
                <ChevronIcon dir="right" />
              </span>
            ) : null}
            <button type="button" class={s.id ? "k-crumb-step m-id" : "k-crumb-step"} aria-current={here ? "page" : undefined} onClick={here ? undefined : s.go}>
              {s.label}
            </button>
          </Fragment>
        );
      })}
    </nav>
  );
}
