// A section of /settings: a display heading, one plain line saying what
// it is, and the rest of what there is to know behind More (a <details>),
// then what it holds. Its id is the section index's anchor (../pages/settings).

import type { ComponentChildren } from "preact";

export function Section({
  id,
  title,
  lede,
  more,
  children,
}: {
  id: string;
  title: string;
  lede: ComponentChildren;
  more?: ComponentChildren;
  children?: ComponentChildren;
}) {
  return (
    <section class="settings-sec" id={id} aria-labelledby={`${id}-h`} data-settings-section={id}>
      <h2 id={`${id}-h`}>{title}</h2>
      <p class="settings-lede">{lede}</p>
      {more ? (
        <details class="settings-more">
          <summary>More</summary>
          {more}
        </details>
      ) : null}
      {children}
    </section>
  );
}
