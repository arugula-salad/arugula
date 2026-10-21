// The session's tabs, over the panes: each tab, a past cloud run shown
// beside them, and + (with ▾ for other kinds of tab). Many tabs scroll
// sideways (the wheel too), the shown one is kept in sight, and + and ▾
// stay at the end (M113).

import { Fragment } from "preact";
import { useLayoutEffect, useRef } from "preact/hooks";
import { cloudPanes, useCloudPanes } from "../../../shared/cloud";
import { paneIds, tabLabel, type Client } from "../../client";
import { directory } from "../../hosts";
import type { Session, TabView } from "../../../shared/proto";
import { AttentionBadge, tabAttention } from "../attention";
import { CloudTabs } from "../../../platform/cloud/cloud";
import { machineState, newTabItems, tabItems } from "../commands";
import { drag, startDrag } from "../drag";
import { useSubscribe } from "../../../shared/ui/hooks";
import { getFleet } from "../hosts";
import { openFresh } from "../open-fresh";
import { drop } from "../panes/drop";
import { TabPeople } from "../people";
import { ariaKeys, keyHint } from "../keys";
import { newTabRefusal } from "./live";
import { RenameInput, type Renaming } from "./rename-input";

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
  const cloud = useCloudPanes();
  const tabKey = keyHint("desktop.tab");
  useSubscribe(drag.subscribe);
  const target = drag.current?.target;
  const marker = target?.kind === "tabbar" ? Math.min(target.index, session.tabs.length) : null;
  const refusal = newTabRefusal(client, session.id);
  // Which ends have tabs scrolled past them, for the fades (style.css).
  const strip = useRef<HTMLDivElement>(null);
  const edges = () => {
    const el = strip.current;
    if (!el) return;
    const more = el.scrollWidth - el.clientWidth > 1;
    const start = more && el.scrollLeft > 1;
    const end = more && el.scrollLeft < el.scrollWidth - el.clientWidth - 1;
    el.dataset.overflow = start && end ? "both" : start ? "start" : end ? "end" : "";
  };
  useLayoutEffect(edges);
  return (
    <div class="tabstrip">
      <div
        ref={strip}
        class="tabbar"
        role="tablist"
        aria-label={`Tabs in ${session.name}`}
        onScroll={edges}
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
                selected={!cloud && id === client.tab}
                renaming={renaming?.kind === "tab" && renaming.id === id}
                setRenaming={setRenaming}
              />
            </Fragment>
          );
        })}
        <CloudTabs />
        {marker === session.tabs.length && <div class="drop-marker" />}
      </div>
      {/* Outside the tabs that scroll, so they're always in reach. */}
      <button
        class="new-tab"
        title={refusal ?? `New tab ${newTabWhere()}${tabKey ? ` (${tabKey})` : ""}`}
        aria-label="New tab"
        aria-keyshortcuts={ariaKeys(tabKey)}
        disabled={!!refusal}
        onClick={() => { cloudPanes.local(); client.intent({ op: "new_tab", session: session.id, from_pane: client.active() ?? null }); }}
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
        disabled={!!refusal}
        data-new-tab-more
        onClick={(e) => {
          const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
          openFresh(client, { clientX: r.left, clientY: r.bottom + 4, preventDefault: () => e.preventDefault() }, () => newTabItems(client, session.id));
        }}
      >
        ▾
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
  const whose = h?.owner ? `, ${h.owner}'s machine` : h?.teamName ? `, team ${h.teamName}'s machine` : "";
  return `on ${name}${whose}`;
}

function TabItem({
  client,
  tab,
  index,
  selected,
  renaming,
  setRenaming,
}: {
  client: Client;
  tab: TabView;
  index: number;
  selected: boolean;
  renaming: boolean;
  setRenaming: (r: Renaming) => void;
}) {
  const label = tabLabel(client, tab);
  const machine = client.tabMachine(tab.id);
  const close = () => client.intent({ op: "close_tab", tab: tab.id });
  // Closing and renaming are an editor's (a viewer's would be refused).
  const edits = client.role(client.sessionOfTab(tab.id) ?? null) !== "viewer";
  const ref = useRef<HTMLDivElement>(null);
  // The shown tab stays in sight when there are more than fit.
  useLayoutEffect(() => {
    if (selected) ref.current?.scrollIntoView?.({ block: "nearest", inline: "nearest" });
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
      class={selected ? "tab selected" : "tab"}
      role="tab"
      aria-selected={selected}
      data-tab-index={index}
      data-tab-id={tab.id}
      title={label}
      onPointerDown={(e) =>
        startDrag(e, { kind: "tab", tab: tab.id }, label, {
          onClick: () => { cloudPanes.local(); client.selectTab(tab.id); },
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
          cloudPanes.local();
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
        <span
          class={`host-tag ${machineState(client, machine)}`}
          title={`${machine.name ?? machine.sprite} (${machine.sprite}): ${machineState(client, machine)}; deleted with the tab`}
        >
          VM
        </span>
      ) : (
        paneIds(tab).some((p) => client.machine(p)) && (
          <span class="host-tag" title="A pane here runs on a throwaway VM">
            VM
          </span>
        )
      )}
      <RemoteTag client={client} tab={tab} />
      <span class="tab-label">{label}</span>
      <TabPeople client={client} tab={tab.id} />
      <AttentionBadge {...tabAttention(client, tab)} client={client} />
      {edits && (
        <button
          class="tab-close"
          title="Close tab"
          aria-label={`Close tab ${label}`}
          onPointerDown={(e) => e.stopPropagation()}
          onClick={(e) => {
            e.stopPropagation();
            close();
          }}
        >
          ×
        </button>
      )}
    </div>
  );
}

/** The other hosts a tab's panes run on (#17). */
function RemoteTag({ client, tab }: { client: Client; tab: TabView }) {
  const hosts = [
    ...new Set(
      paneIds(tab)
        .map((p) => client.blocks.get(p)?.state as { host?: string } | null | undefined)
        .flatMap((s) => (s?.host ? [s.host] : [])),
    ),
  ];
  if (!hosts.length) return null;
  return (
    <span class="host-tag remote" title={`Runs on ${hosts.join(", ")}`}>
      {hosts.join(" ")}
    </span>
  );
}
