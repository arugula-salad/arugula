// The session's tabs, over the panes: each tab, and + (with ▾ for other
// kinds of tab). Many tabs scroll
// sideways (the wheel too), the shown one is kept in sight, and + and ▾
// stay at the end (M113). Each tab is as wide as its name (within a
// minimum and a maximum), and when they don't all fit a menu after them
// lists every one (UI audit #171).

import { Fragment } from "preact";
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
import { ariaKeys, keyHint } from "../keys";
import { newTabRefusal } from "./live";
import { RenameInput, type Renaming } from "./rename-input";
import { HOSTED_LABEL } from "../../../shared/hosted";
import { openMenu, type MenuItem } from "../../../shared/ui/menu";
import { Chip } from "../../../shared/ui/state-chip";
import { Caret, ChevronIcon, CloseIcon, IconButton, StarIcon } from "../../../shared/ui/icons";
import { starOf } from "../../stars";

/** The tab bar of a session. */
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
  // The shown tab's close button sits beside the tab list, not in it (a
  // tab list holds only tabs): laid over the room the tab keeps for it.
  const closeBtn = useRef<HTMLButtonElement>(null);
  const shown = client.tab;
  const closing = shown !== null && session.tabs.includes(shown) && client.role(session.id) !== "viewer" && !(renaming?.kind === "tab" && renaming.id === shown);
  const placeClose = () => {
    const btn = closeBtn.current;
    const bar = strip.current;
    const tab = bar?.querySelector<HTMLElement>(".tab.closable");
    if (!btn || !bar || !tab) return;
    const at = tab.getBoundingClientRect();
    const box = bar.getBoundingClientRect();
    const css = getComputedStyle(tab);
    // Over the tab's 20px slot (::after), 24px wide so it can be hit.
    const right = at.right - parseFloat(css.paddingRight) - parseFloat(css.borderRightWidth) - 20 - 2;
    const host = (btn.offsetParent as HTMLElement | null)?.getBoundingClientRect() ?? box;
    btn.style.left = `${right - host.left}px`;
    btn.style.top = `${at.top + at.height / 2 - 12 - host.top}px`;
    // Scrolled out of the list's sight, it goes with the tab.
    btn.hidden = right < box.left || right + 24 > box.right;
  };
  useLayoutEffect(() => {
    edges();
    placeClose();
  });
  // A window made narrower can hide tabs without a render.
  useLayoutEffect(() => {
    const el = strip.current;
    if (!el || typeof ResizeObserver === "undefined") return;
    const ro = new ResizeObserver(() => {
      edges();
      placeClose();
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, []);
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
  return (
    <div class="tabstrip">
      <div
        ref={strip}
        class="tabbar"
        role="tablist"
        aria-label={`Tabs in ${session.name}`}
        onScroll={() => {
          edges();
          placeClose();
        }}
        onWheel={(e) => {
          // A mouse wheel scrolls the tabs sideways.
          const el = e.currentTarget as HTMLElement;
          if (Math.abs(e.deltaY) > Math.abs(e.deltaX) && el.scrollWidth > el.clientWidth) {
            el.scrollLeft += e.deltaY;
            e.preventDefault();
          }
        }}
      >
        {session.tabs.map((id, i) => {
          const t = client.tabView(id);
          if (!t) return null;
          return (
            <Fragment key={id}>
              {marker === i && <div class="drop-marker" />}
              <TabItem
                client={client}
                tab={t}
                index={i}
                selected={id === client.tab}
                closable={id === shown && closing}
                renaming={renaming?.kind === "tab" && renaming.id === id}
                setRenaming={setRenaming}
              />
            </Fragment>
          );
        })}
        {marker === session.tabs.length && <div class="drop-marker" />}
      </div>
      {closing && (
        <IconButton label={`Close tab ${tabLabel(client, client.tabView(shown)!)}`} title="Close tab" ref={closeBtn} class="tab-close" onClick={() => confirmCloseTab(client, shown)}>
          <CloseIcon size="xs" />
        </IconButton>
      )}
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

function TabItem({
  client,
  tab,
  index,
  selected,
  closable,
  renaming,
  setRenaming,
}: {
  client: Client;
  tab: TabView;
  index: number;
  selected: boolean;
  closable: boolean;
  renaming: boolean;
  setRenaming: (r: Renaming) => void;
}) {
  const label = tabLabel(client, tab);
  const machine = client.tabMachine(tab.id);
  const close = () => confirmCloseTab(client, tab.id);
  // Renaming is an editor's (a viewer's would be refused).
  const edits = client.role(client.sessionOfTab(tab.id) ?? null) !== "viewer";
  const ref = useRef<HTMLDivElement>(null);
  // The shown tab stays in sight when there are more than fit: tabs are as
  // wide as their names (UI audit #171), and names arrive after a tab does,
  // so it's put back in sight whenever a tab in the strip changes size.
  useLayoutEffect(() => {
    const el = ref.current;
    if (!selected || !el) return;
    const show = () => el.scrollIntoView?.({ block: "nearest", inline: "nearest" });
    show();
    const strip = el.parentElement;
    if (!strip || typeof ResizeObserver === "undefined") return;
    const ro = new ResizeObserver(show);
    for (const t of strip.children) ro.observe(t);
    return () => ro.disconnect();
  }, [selected]);
  if (renaming) {
    return (
      <div class="tab selected">
        <RenameInput
          value={tab.name ?? label}
          onDone={(name) => {
            setRenaming(null);
            if (name !== null) client.intent({ op: "rename_tab", tab: tab.id, name: name || null });
          }}
        />
      </div>
    );
  }
  return (
    <div
      ref={ref}
      class={closable ? "tab selected closable" : selected ? "tab selected" : "tab"}
      role="tab"
      aria-selected={selected}
      data-tab-index={index}
      data-tab-id={tab.id}
      title={label}
      onPointerDown={(e) =>
        startDrag(e, { kind: "tab", tab: tab.id }, label, {
          onClick: () => client.selectTab(tab.id),
          onDrop: (what, target) => drop(client, what, target),
        })
      }
      // From the keyboard (M113): the shown tab is the one Tab reaches;
      // arrows, Home and End move between tabs, Enter or Space shows one,
      // F2 renames it, and the menu key opens its menu.
      tabIndex={selected ? 0 : -1}
      onKeyDown={(e) => {
        const el = e.currentTarget as HTMLElement;
        const tabs = [...(el.parentElement?.querySelectorAll<HTMLElement>(".tab[data-tab-id]") ?? [])];
        const i = tabs.indexOf(el);
        const go = (j: number) => tabs[(j + tabs.length) % tabs.length]?.focus();
        if (e.key === "ArrowRight") go(i + 1);
        else if (e.key === "ArrowLeft") go(i - 1);
        else if (e.key === "Home") go(0);
        else if (e.key === "End") go(tabs.length - 1);
        else if (e.key === "Enter" || e.key === " ") {
          client.selectTab(tab.id);
        } else if (e.key === "F2" && edits) setRenaming({ kind: "tab", id: tab.id });
        else return;
        e.preventDefault();
      }}
      onDblClick={() => edits && setRenaming({ kind: "tab", id: tab.id })}
      onAuxClick={(e) => e.button === 1 && edits && close()}
      onContextMenu={(e) => openFresh(client, e, () => tabItems(client, tab, () => setRenaming({ kind: "tab", id: tab.id })))}
    >
      {machine ? (
        <Chip
          kind="tag"
          tone={machineTone(machineState(client, machine))}
          class="chip-gap"
          title={`${machine.name ?? machine.sprite} (${machine.sprite}): ${machineState(client, machine)}; deleted with the tab`}
        >
          VM
        </Chip>
      ) : (
        paneIds(tab).some((p) => client.machine(p)) && (
          <Chip kind="tag" tone="info" class="chip-gap" title="A pane here runs on a throwaway VM">
            VM
          </Chip>
        )
      )}
      {starOf(client.state?.options, "tab", tab.id) !== null && (
        <span class="star-mark" role="img" aria-label="Starred tab" title="Starred tab: it's in Starred in the Sessions sidebar" data-tab-star>
          <StarIcon on size="xs" />
        </span>
      )}
      <span class="tab-label">{label}</span>
      <TabPeople client={client} tab={tab.id} />
      <AttentionBadge {...tabAttention(client, tab)} client={client} />
      <ChatTabBadge client={client} panes={paneIds(tab)} />
    </div>
  );
}

