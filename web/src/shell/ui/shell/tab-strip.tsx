// The session's tabs, over the panes: each tab, and + (with ▾ for other
// kinds of tab). Many tabs scroll
// sideways (the wheel too), the shown one is kept in sight, and + and ▾
// stay at the end (M113). Each tab is as wide as its name (within a
// minimum and a maximum), and when they don't all fit a menu after them
// lists every one (UI audit #171).

import { useLayoutEffect, useRef, useState } from "preact/hooks";
import { paneIds, tabLabel, type Client } from "../../client";
import { confirmCloseTab } from "../close-tab";
import { directory } from "../../hosts";
import type { Session, TabView } from "../../../shared/proto";
import { AttentionBadge, tabAttention } from "../attention";
import { ChatTabBadge } from "../../blocks";
import { machineState, machineTone, newTabItems, tabItems } from "../commands";
import { drag, startDrag } from "../drag";
import { useSubscribe } from "../../../shared/ui/hooks";
import { getFleet } from "../hosts";
import { openFresh } from "../open-fresh";
import { drop } from "../panes/drop";
import { TabPeople } from "../people";
import { ariaKeys, keyHint } from "../../../shared/keys";
import { newTabRefusal } from "./live";
import { SidebarToggle } from "./context-header";
import { type Renaming } from "./rename-input";
import { Tabs, type TabItem } from "../../../shared/kit/tabs";
import type { State } from "../../../shared/kit/state-mark";
import { HOSTED_LABEL } from "../../../shared/hosted";
import { openMenu, type MenuItem } from "../../../shared/ui/menu";
import { Chip } from "../../../shared/ui/state-chip";
import { Caret, ChevronIcon } from "../../../shared/ui/icons";

/** The tab bar of a session, drawn with the kit's Tabs (Lift 6a, #1483):
 * each tab its state mark, its name (renamed in place), who is in it and
 * its badges; a drag moves it, or drops a pane into the bar; the shown
 * one's × closes it. */
export function TabStrip({
  client,
  session,
  renaming,
  setRenaming,
}: {
  client: Client;
  session: Session;
  renaming: Renaming;
  setRenaming: (r: Renaming) => void;
}) {
  const tabKey = keyHint("desktop.tab");
  useSubscribe(drag.subscribe);
  const target = drag.current?.target;
  const marker = target?.kind === "tabbar" ? Math.min(target.index, session.tabs.length) : null;
  const refusal = newTabRefusal(client, session.id);
  // Which ends have tabs scrolled past them, for the fades (style.css).
  const strip = useRef<HTMLDivElement>(null);
  const [over, setOver] = useState(false);
  const edges = () => {
    const el = strip.current;
    if (!el) return;
    const more = el.scrollWidth - el.clientWidth > 1;
    const start = more && el.scrollLeft > 1;
    const end = more && el.scrollLeft < el.scrollWidth - el.clientWidth - 1;
    el.dataset.overflow = start && end ? "both" : start ? "start" : end ? "end" : "";
    setOver(more);
  };
  const shown = client.tab;
  const closing = shown !== null && session.tabs.includes(shown) && client.role(session.id) !== "viewer" && !(renaming?.kind === "tab" && renaming.id === shown);
  useLayoutEffect(() => edges());
  // A window made narrower can hide tabs without a render.
  useLayoutEffect(() => {
    const el = strip.current;
    if (!el || typeof ResizeObserver === "undefined") return;
    const ro = new ResizeObserver(edges);
    ro.observe(el);
    return () => ro.disconnect();
  }, []);
  // The shown tab stays in sight when there are more than fit: tabs are as
  // wide as their names, which arrive after a tab does, so it's put back in
  // sight whenever a tab in the strip changes size.
  useLayoutEffect(() => {
    const el = strip.current;
    const show = () => el?.querySelector<HTMLElement>(`[data-tab-id="${shown}"]`)?.scrollIntoView?.({ block: "nearest", inline: "nearest" });
    show();
    if (!el || typeof ResizeObserver === "undefined") return;
    const ro = new ResizeObserver(show);
    for (const t of el.children) ro.observe(t);
    return () => ro.disconnect();
  }, [shown, session.tabs.length]);
  /** Every tab, the shown one checked: those scrolled out of sight too. */
  const allTabs = (e: MouseEvent) => {
    const el = e.currentTarget as HTMLElement;
    const r = el.getBoundingClientRect();
    const items = session.tabs.flatMap((id): MenuItem[] => {
      const t = client.tabView(id);
      return t ? [{ label: tabLabel(client, t), checked: id === client.tab, run: () => client.selectTab(id) }] : [];
    });
    openMenu({ clientX: r.left, clientY: r.bottom + 4, preventDefault: () => e.preventDefault() }, [{ header: `${items.length} tabs` }, ...items], { from: el });
  };
  const items = session.tabs.flatMap((id, i): TabItem[] => {
    const t = client.tabView(id);
    return t ? [tabItem(client, t, i, id === shown && closing, renaming?.kind === "tab" && renaming.id === id, setRenaming)] : [];
  });
  return (
    <div class="tabstrip">
      <SidebarToggle client={client} />
      <Tabs
        label={`Tabs in ${session.name}`}
        tabs={items}
        current={String(shown)}
        onPick={(id) => client.selectTab(Number(id))}
        onClose={(id) => confirmCloseTab(client, Number(id))}
        barRef={strip}
        marker={marker}
        manual
        bar={{
          "data-tab-bar": "",
          onScroll: edges,
          onWheel: (e) => {
            // A mouse wheel scrolls the tabs sideways.
            const el = e.currentTarget as HTMLElement;
            if (Math.abs(e.deltaY) > Math.abs(e.deltaX) && el.scrollWidth > el.clientWidth) {
              el.scrollLeft += e.deltaY;
              e.preventDefault();
            }
          },
        }}
      />
      {over && (
        <button class="tabs-all" title="Every tab in this session" aria-label={`All ${session.tabs.length} tabs`} aria-haspopup="menu" aria-expanded="false" data-tabs-all onClick={allTabs}>
          {session.tabs.length}
          <Caret />
        </button>
      )}
      {/* Outside the tabs that scroll, so they're always in reach. */}
      <button
        class="new-tab"
        title={refusal ?? `New tab ${newTabWhere()}${tabKey ? ` (${tabKey})` : ""}`}
        aria-label="New tab"
        aria-keyshortcuts={ariaKeys(tabKey)}
        disabled={!!refusal}
        onClick={() => client.intent({ op: "new_tab", session: session.id, from_pane: client.active() ?? null })}
        onContextMenu={(e) => !refusal && openFresh(client, e, () => newTabItems(client, session.id))}
      >
        +
      </button>
      {/* What else a new tab can be (a directory, a PR, a VM…), on a
          click: not only behind a right-click. */}
      <button
        class="new-tab-more"
        title={refusal ?? "More kinds of tab: a directory, a PR, a VM…"}
        aria-label="More kinds of tab"
        aria-haspopup="menu"
        aria-expanded="false"
        disabled={!!refusal}
        data-new-tab-more
        onClick={(e) => {
          const el = e.currentTarget as HTMLElement;
          const r = el.getBoundingClientRect();
          openFresh(client, { clientX: r.left, clientY: r.bottom + 4, preventDefault: () => e.preventDefault() }, () => newTabItems(client, session.id), { from: el });
        }}
      >
        <ChevronIcon />
      </button>
    </div>
  );
}

/** A tab's state as its mark: it asks you, it works, it's done, or it's quiet. */
function tabMark(client: Client, tab: TabView): State {
  const a = tabAttention(client, tab).state;
  if (a === "needs_input") return "asks";
  if (a === "done") return "done";
  return paneIds(tab).some((p) => client.info(p)?.attention === "working") ? "working" : "idle";
}

/** One tab as the kit's Tabs draws it: its drag, its menu, F2 and a double
 * click to rename (an editor's), a middle click to close, and who and what
 * is in it after its name. */
function tabItem(client: Client, tab: TabView, index: number, closes: boolean, renaming: boolean, setRenaming: (r: Renaming) => void): TabItem {
  const label = tabLabel(client, tab);
  const machine = client.tabMachine(tab.id);
  // Renaming is an editor's (a viewer's would be refused).
  const edits = client.role(client.sessionOfTab(tab.id) ?? null) !== "viewer";
  const rename = edits ? () => setRenaming({ kind: "tab", id: tab.id }) : undefined;
  return {
    id: String(tab.id),
    title: label,
    mark: tabMark(client, tab),
    closes,
    rename,
    renaming: renaming
      ? {
          value: tab.name ?? label,
          done: (name) => {
            setRenaming(null);
            if (name !== null) client.intent({ op: "rename_tab", tab: tab.id, name: name || null });
          },
        }
      : undefined,
    extra: (
      <>
        {machine ? (
          <Chip kind="tag" tone={machineTone(machineState(client, machine))} class="chip-gap" title={`${machine.name ?? machine.sprite} (${machine.sprite}): ${machineState(client, machine)}; deleted with the tab`}>
            VM
          </Chip>
        ) : (
          paneIds(tab).some((p) => client.machine(p)) && (
            <Chip kind="tag" tone="info" class="chip-gap" title="A pane here runs on a throwaway VM">
              VM
            </Chip>
          )
        )}
        <TabPeople client={client} tab={tab.id} />
        <AttentionBadge {...tabAttention(client, tab)} client={client} />
        <ChatTabBadge client={client} panes={paneIds(tab)} />
      </>
    ),
    attrs: {
      "data-tab-id": tab.id,
      "data-tab-index": index,
      title: label,
      onPointerDown: (e: PointerEvent) =>
        startDrag(e, { kind: "tab", tab: tab.id }, label, {
          onClick: () => client.selectTab(tab.id),
          onDrop: (what, target) => drop(client, what, target),
        }),
      onDblClick: () => rename?.(),
      onAuxClick: (e: MouseEvent) => e.button === 1 && edits && confirmCloseTab(client, tab.id),
      onContextMenu: (e: MouseEvent) => openFresh(client, e, () => tabItems(client, tab, () => setRenaming({ kind: "tab", id: tab.id }))),
    } as TabItem["attrs"],
  };
}

/** Where + opens a tab (#551): the shown machine, and whose it is, since
 * on someone else's it runs as them. */
function newTabWhere(): string {
  const name = directory.current;
  if (!name) return "here";
  const h = getFleet()?.host(name);
  const whose = h?.hosted
    ? `, ${HOSTED_LABEL.toLowerCase()}`
    : h?.owner ? `, ${h.owner}'s machine` : h?.teamName ? `, team ${h.teamName}'s machine` : "";
  return `on ${name}${whose}`;
}
