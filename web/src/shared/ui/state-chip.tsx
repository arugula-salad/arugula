// Chips, in five shapes (UI audit #183, #730), on the platform page, the
// shell and control's. A state (a run's, a deploy's, an issue's, a
// workspace's) is a `StateChip`: the chip with a dot, in the tone its
// card's bar takes in a list (../states.ts), its word capitalised wherever
// it shows. A label is a `Chip kind="tag"`, a number a `Count`, a thing
// with an id (an issue, a board, a setting, a file) an `ObjectChip`, and a
// tip about a pane a `HintChip`. The looks are ./state-chip.css.

import type { ComponentChildren } from "preact";
import "./state-chip.css";

/** A chip's tone, by the palette color it takes (as a card's bar does,
 * platform/swarm.css). */
export type Tone = "accent" | "ok" | "warn" | "danger" | "info" | "teal" | "merged" | "muted";

type Data = { [data: `data-${string}`]: unknown };

/** A chip: `tone` colors it (muted when none), `dot` marks a state, and
 * `kind="tag"` makes it a label: a raised box, with no edge. With `onClick`
 * it's a button. Anything else (`data-*`, a title) lands on it. */
export function Chip({
  tone,
  dot,
  kind,
  class: cls,
  title,
  onClick,
  children,
  ...attrs
}: {
  tone?: Tone | null;
  dot?: boolean;
  kind?: "tag";
  class?: string;
  title?: string;
  onClick?: (e: MouseEvent) => void;
  children: ComponentChildren;
} & Data) {
  const c = `chip chip-${tone ?? "muted"}${kind === "tag" ? " chip-tag" : ""}${dot ? " chip-dot" : ""}${cls ? ` ${cls}` : ""}`;
  return onClick ? (
    <button type="button" class={c} title={title} onClick={onClick} {...attrs}>
      {children}
    </button>
  ) : (
    <span class={c} title={title} {...attrs}>
      {children}
    </span>
  );
}

/** A state, as one chip everywhere: a dot and its word in its tone. */
export function StateChip({ tone, children, ...rest }: { tone: Tone | null; class?: string; title?: string; children: ComponentChildren } & Data) {
  return (
    <Chip tone={tone} dot {...rest}>
      {children}
    </Chip>
  );
}

/** A count: neutral, `for="you"` when it's unread or yours (accent), and
 * `for="mention"` when it names you (warn). With `onClick` it's a button. */
export function Count({
  for: who,
  class: cls,
  title,
  onClick,
  children,
  ...attrs
}: {
  for?: "you" | "mention";
  class?: string;
  title?: string;
  onClick?: (e: MouseEvent) => void;
  children: ComponentChildren;
} & Data) {
  const c = `count${who ? ` count-${who}` : ""}${cls ? ` ${cls}` : ""}`;
  return onClick ? (
    <button type="button" class={c} title={title} onClick={onClick} {...attrs}>
      {children}
    </button>
  ) : (
    <span class={c} title={title} {...attrs}>
      {children}
    </span>
  );
}

/** A thing with an id: `id` in mono, then its title (the children), then a
 * `fact` in its tone. `proof` draws it dashed, `pair` as two cells (`id`,
 * then `fact`), `onRemove` adds a ✕. It's a link with `href` (`target` and
 * `rel` are the caller's), a button with
 * `onClick` (keyboard-reachable, `pressed` and `disabled` as a toggle's),
 * else plain; anything else (`data-*`, drag handlers) lands on it. */
export function ObjectChip({
  id,
  fact,
  proof,
  pair,
  href,
  onClick,
  onRemove,
  removeLabel,
  removeAttrs,
  pressed,
  disabled,
  class: cls,
  title,
  children,
  ...attrs
}: {
  id?: ComponentChildren;
  fact?: { tone: Tone | null; text: ComponentChildren } | null;
  proof?: boolean;
  pair?: boolean;
  href?: string;
  onClick?: (e: MouseEvent) => void;
  onRemove?: (e: MouseEvent) => void;
  /** The ✕'s accessible name. */
  removeLabel?: string;
  /** `data-*` on the ✕. */
  removeAttrs?: Data;
  pressed?: boolean;
  disabled?: boolean;
  class?: string;
  title?: string;
  children?: ComponentChildren;
} & Data &
  Record<string, unknown>) {
  const c = `chip-object${proof ? " proof" : ""}${pair ? " pair" : ""}${cls ? ` ${cls}` : ""}`;
  const body = (
    <>
      {id != null && <code class="chip-object-id">{id}</code>}
      {children}
      {fact && <span class={`chip-object-fact chip-${fact.tone ?? "muted"}`}>{fact.text}</span>}
      {onRemove && (
        <button type="button" class="chip-object-x" data-chip-remove aria-label={removeLabel ?? "Remove"} title={removeLabel ?? "Remove"} onClick={onRemove} {...removeAttrs}>
          ✕
        </button>
      )}
    </>
  );
  if (href) {
    return (
      <a class={c} href={href} title={title} {...attrs}>
        {body}
      </a>
    );
  }
  if (onClick) {
    return (
      <button type="button" class={c} title={title} onClick={onClick} aria-pressed={pressed} disabled={disabled} {...attrs}>
        {body}
      </button>
    );
  }
  return (
    <span class={c} title={title} {...attrs}>
      {body}
    </span>
  );
}

/** A tip about a pane: its `copy`, an action and ×. The shell says where it
 * shows (hints.tsx); `class` places it. */
export function HintChip({
  id,
  copy,
  action,
  onAct,
  onDismiss,
  class: cls,
}: {
  id: string;
  copy: ComponentChildren;
  action?: string;
  onAct: () => void;
  onDismiss: () => void;
  class?: string;
}) {
  return (
    <span class={`hint${cls ? ` ${cls}` : ""}`} role="status" data-hint={id}>
      <span class="hint-copy">{copy}</span>
      {action && (
        <button class="hint-act" data-hint-act onClick={onAct}>
          {action}
        </button>
      )}
      <button class="hint-dismiss" data-hint-dismiss aria-label="Dismiss this tip" title="Dismiss (Tips in Settings turns them all off)" onClick={onDismiss}>
        ×
      </button>
    </span>
  );
}
