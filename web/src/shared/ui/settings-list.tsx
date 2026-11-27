// Settings as one list a scope (Lift 7, #1487): its header (the scopes and
// which one), then each of its sections a kit Row (./settings-section.tsx)
// with its title and what's set, opening in place under its line. The open
// one is the address's, so a link opens it; a click on another opens it
// there, and on the open one closes it. The same list on a phone.
//
// Every section of the scope is drawn, the open one shown and the rest
// closed, so each keeps reading what it reads and its row's summary stays
// current. One that draws nothing (Claude, until there's something in it)
// has no row.

import type { ComponentChildren } from "preact";
import { useEffect, useMemo } from "preact/hooks";
import { Surface } from "../kit/surface";
import { SectionListContext, type SectionList } from "./settings-section";

export function SettingsList({
  label,
  header,
  section,
  hrefOf,
  go,
  children,
  ...attrs
}: {
  /** What the list is, for assistive tech: "Settings". */
  label: string;
  /** Over the rows: the page's title, its scopes and picker. */
  header: ComponentChildren;
  /** The section the address names, if it names one. */
  section?: string;
  /** A section's address; `null`, the list's own (none open). */
  hrefOf: (section: string | null) => string;
  /** Going to an address without a reload. */
  go: (href: string) => void;
  /** The scope's sections. */
  children: ComponentChildren;
  [hook: `data-${string}`]: string | undefined;
}) {
  const list: SectionList = useMemo(() => ({ asked: section ?? null, toggle: (id) => go(hrefOf(id === section ? null : id)) }), [section, hrefOf, go]);
  // A section opened by its address comes into view once it's drawn.
  useEffect(() => {
    if (section) document.getElementById(section)?.scrollIntoView?.({ block: "nearest" });
  }, [section]);
  return (
    <Surface kind="section" as="section" class="settings-list" aria-label={label} data-settings-list {...attrs}>
      <header class="settings-head">{header}</header>
      <div class="settings-sections">
        <SectionListContext.Provider value={list}>{children}</SectionListContext.Provider>
      </div>
    </Surface>
  );
}
