// The session's tabs, in the bar: each tab, the cloud runs opened beside
// them, and + (with ▾ for other kinds of tab).

import { Fragment } from "preact";
import { cloudPanes, useCloudPanes } from "../../cloud";
import { paneIds, tabLabel, type Client } from "../../client";
import { directory } from "../../hosts";
import type { Session, TabView } from "../../proto";
import { AttentionBadge, tabAttention } from "../attention";
import { CloudTabs } from "../cloud";
import { machineState, newTabItems, tabItems } from "../commands";
import { drag, startDrag } from "../drag";
import { useSubscribe } from "../hooks";
import { getFleet } from "../hosts";
import { openFresh } from "../open-fresh";
import { drop } from "../panes/drop";
import { TabPeople } from "../people";
import { ariaKeys, keyHint } from "../keys";
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
  return (
    <div class="tabbar" role="tablist">
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
      <button
        class="new-tab"
        title={`New tab ${newTabWhere()}${tabKey ? ` (${tabKey})` : ""}`}
        aria-label="New tab"
        aria-keyshortcuts={ariaKeys(tabKey)}
        onClick={() => { cloudPanes.local(); client.intent({ op: "new_tab", session: session.id, from_pane: client.active() ?? null }); }}
        onContextMenu={(e) => openFresh(client, e, () => newTabItems(client, session.id))}
      >
        +
      </button>
      {/* What else a new tab can be (a directory, a PR, a VM…), on a
          click: not only behind a right-click. */}
      <button
        class="new-tab-more"
        title="More kinds of tab"
        aria-label="More kinds of tab"
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
      onDblClick={() => setRenaming({ kind: "tab", id: tab.id })}
      onAuxClick={(e) => e.button === 1 && close()}
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
