// Over the panes. The classic bar is the shared app bar (machine, places,
// commands, account) with the session, Share and the tab strip in it. The
// new workspace's context header (M108, #1023) says where you are: the
// machine and how it's reached, the session, then command search, the
// people here, the account and the window's buttons. It's the desktop app's
// titlebar either way.

import { tabLabel, type Client } from "../../client";
import { directory } from "../../hosts";
import { AccountButton, AppBar, CommandsButton } from "../app-bar";
import { CloudTabs } from "../cloud";
import { useSubscribe } from "../hooks";
import { getFleet, HostButton, hostMenuShown, machineState } from "../hosts";
import { PeopleBar, shareSession } from "../people";
import { UpdateChip } from "../update";
import { WindowButtons } from "../window-buttons";
import type { Renaming } from "./rename-input";
import { SessionButton } from "./session-sidebar";
import { TabStrip } from "./tab-strip";

/** The classic bar (the default until M113). */
export function ClassicBar({
  client,
  renaming,
  setRenaming,
  inert,
}: {
  client: Client;
  renaming: Renaming;
  setRenaming: (r: Renaming) => void;
  inert: boolean;
}) {
  const state = client.state!;
  // None at all, on a machine with no sessions: the bar is still here for
  // the host and account menus.
  const session = state.sessions.find((s) => s.id === client.session) ?? state.sessions[0];
  return (
    <AppBar client={client} at="panes" inert={inert} end={<PeopleBar client={client} />}>
      {session && (
        <>
          <SessionButton client={client} session={session} renaming={renaming} setRenaming={setRenaming} />
          {/* Sharing is the daemon's owner's (M13), and on the bar so it's found (#551). */}
          {state.roles ? null : (
            <button class="share-button" aria-label="Share session" title="Share this session with someone" onClick={() => shareSession(session.id)}>
              Share
            </button>
          )}
          <TabStrip client={client} session={session} renaming={renaming} setRenaming={setRenaming} />
        </>
      )}
      {!session && <div class="tabbar" role="tablist"><CloudTabs /></div>}
    </AppBar>
  );
}

export type Connection = "direct" | "relayed" | "reconnecting" | "offline";

/** How the shown machine is reached, from real state: the page's own
 * connection first (direct or through control's relay), then, while it's
 * down, what control and the fleet say of the machine. */
export function connection(client: Client): { state: Connection; why: string } {
  if (client.connected) {
    return client.path === "relayed"
      ? { state: "relayed", why: "Through Arugula control's relay (end to end encrypted)" }
      : { state: "direct", why: "Straight to the machine" };
  }
  const name = directory.current;
  const said = name ? machineState(name) : "";
  if (said.startsWith("offline")) return { state: "offline", why: `${name} is ${said}; still trying` };
  return { state: "reconnecting", why: `Lost the connection to ${name || "the machine"}; reconnecting${client.failures ? ` (${client.failures} tries)` : ""}` };
}

const CONNECTION_WORDS: Record<Connection, string> = {
  direct: "Direct",
  relayed: "Relayed",
  reconnecting: "Reconnecting",
  offline: "Offline",
};

export function ConnectionChip({ client }: { client: Client }) {
  useSubscribe((fn) => directory.subscribe(fn));
  useSubscribe((fn) => getFleet()?.subscribe(fn) ?? (() => {}));
  const { state, why } = connection(client);
  return (
    <span class={`ws-conn ${state}`} data-connection={state} role="status" title={why}>
      <span class="ws-dot" aria-hidden="true" />
      {CONNECTION_WORDS[state]}
    </span>
  );
}

/** The new workspace's header. */
export function ContextHeader({
  client,
  renaming,
  setRenaming,
  inert,
}: {
  client: Client;
  renaming: Renaming;
  setRenaming: (r: Renaming) => void;
  inert: boolean;
}) {
  useSubscribe((fn) => directory.subscribe(fn));
  const state = client.state!;
  const session = state.sessions.find((s) => s.id === client.session) ?? state.sessions[0];
  const tab = client.tabView();
  return (
    <header class="ws-head" data-tauri-drag-region inert={inert}>
      <nav class="ws-crumbs" aria-label="Where you are" data-tauri-drag-region>
        {/* The machine's name even where there's no menu of others (a
            daemon's own page alone): HostButton draws nothing then. */}
        {hostMenuShown() ? (
          <HostButton path={false} />
        ) : (
          <span class="ws-host-name" title="This machine">
            {directory.current || "This machine"}
          </span>
        )}
        <ConnectionChip client={client} />
        {session && (
          <>
            <span class="ws-crumb-sep" aria-hidden="true">
              /
            </span>
            <SessionButton client={client} session={session} renaming={renaming} setRenaming={setRenaming} crumb />
          </>
        )}
        {tab && (
          <span class="ws-crumb-tab" title="This tab" data-tauri-drag-region>
            <span class="ws-crumb-sep" aria-hidden="true">
              /
            </span>
            <span class="ws-crumb-tab-name">{tabLabel(client, tab)}</span>
          </span>
        )}
      </nav>
      <div class="bar-fill" data-tauri-drag-region />
      <CommandsButton client={client} search />
      <UpdateChip client={client} />
      {/* Sharing is the daemon's owner's (M13), and in sight so it's found (#551). */}
      {session && !state.roles && (
        <button class="ws-share" aria-label="Share session" title="Share this session with someone" onClick={() => shareSession(session.id)}>
          Share
        </button>
      )}
      <PeopleBar client={client} />
      <AccountButton client={client} />
      <WindowButtons />
    </header>
  );
}
