// Crumb (kit §4.1, 6): where you are, as a path you can climb. The first
// step is the project; the last is here, and isn't a link.

import { Fragment, type ComponentChildren } from "preact";
import { ChevronIcon } from "../ui/icons";
import "./kit.css";

export interface Step {
  label: string;
  /** Climb to it (the project's opens the project list). */
  go?: () => void;
  /** Its address, when it's a page: a link (a modified click is the browser's). */
  href?: string;
  /** An id or a machine: set in the id face. */
  id?: boolean;
  /** Its `data-path` hook. */
  hook?: string;
  /** A step that is its own control (a machine's menu, a session's). */
  node?: ComponentChildren;
}

export function Crumb({ steps, ...data }: { steps: Step[]; [data: `data-${string}`]: unknown }) {
  return (
    <nav class="k-crumb" aria-label="Where you are" data-kit="crumb" {...data}>
      {steps.map((s, i) => {
        // The last step is here, unless it's a link up (an issue's Issues).
        const here = i === steps.length - 1 && !s.href;
        return (
          <Fragment key={i}>
            {i ? (
              <span class="k-crumb-sep" aria-hidden="true">
                <ChevronIcon dir="right" />
              </span>
            ) : null}
            {s.node ??
              (s.href ? (
                <a class={s.id ? "k-crumb-step m-id" : "k-crumb-step"} href={s.href} data-path={s.hook} onClick={(e) => s.go && !(e.metaKey || e.ctrlKey || e.shiftKey || e.altKey || e.button) && (e.preventDefault(), s.go())}>
                  {s.label}
                </a>
              ) : (
                <button type="button" class={s.id ? "k-crumb-step m-id" : "k-crumb-step"} aria-current={here ? "page" : undefined} data-path={s.hook} onClick={here ? undefined : s.go}>
                  {s.label}
                </button>
              ))}
          </Fragment>
        );
      })}
    </nav>
  );
}
