// Over the panes, the workspace's context header (M108, #1023) says where
// you are: the machine and how it's reached, the session and the tab, then
// command search, the bell, Share, the people here, the account and the
// window's buttons. It's the desktop app's titlebar.

import { tabLabel, type Client } from "../../client";
import { directory } from "../../hosts";
import { AccountButton, CommandsButton } from "../app-bar";
import { useSubscribe } from "../../../shared/ui/hooks";
import { getFleet, HostButton, hostMenuShown, machineState } from "../hosts";
import { PeopleBar, shareSession } from "../people";
import { UpdateChip } from "../update";
import { WindowButtons } from "../window-buttons";
import type { Renaming } from "./rename-input";
import { SessionButton } from "./session-sidebar";
import { AttentionBell } from "./attention-drawer";
import { offlineWhy } from "./live";
import { useSidebar } from "./prefs";
import { openOverview } from "../places";
import { cloudWorkspaceRoute } from "../../../shared/cloud";
import { CloudWorkspaceCrumbs, DeleteCloudWorkspace, useCloudWorkspaces } from "../../../platform/cloud/workspaces";
import { RebuildCloudWorkspace } from "../../../platform/cloud/workspace-rebuild";

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

/** The workspace's header. */
export function ContextHeader({
  client,
  setRenaming,
  inert,
}: {
  client: Client;
  setRenaming: (r: Renaming) => void;
  inert: boolean;
}) {
  useSubscribe((fn) => directory.subscribe(fn));
  useCloudWorkspaces();
  const sidebar = useSidebar();
  if (cloudWorkspaceRoute()) return <CloudWorkspaceHeader client={client} inert={inert} />;
  const state = client.state!;
  const session = state.sessions.find((s) => s.id === client.session) ?? state.sessions[0];
  const tab = client.tabView();
  return (
    <header class="ws-head" data-tauri-drag-region inert={inert}>
      {sidebar.open && !sidebar.narrow && (
        <button class="ws-wordmark" style={{ width: `${sidebar.width}px` }}
          aria-label="Arugula Control: session overview" title="This machine’s session overview"
          onClick={() => openOverview()}>
          Arugula <span aria-hidden="true">/</span> <strong>Control</strong>
        </button>
      )}
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
            <SessionButton client={client} session={session} setRenaming={setRenaming} />
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
      {/* M111: what waits on you, on every machine. */}
      <AttentionBell client={client} />
      {/* Sharing is the daemon's owner's (M13), and in sight so it's found (#551). */}
      {session && !state.roles && (
        <button
          class="ws-share"
          aria-label="Share session"
          title={offlineWhy(client) ?? "Share this session with someone"}
          disabled={!client.connected}
          onClick={() => shareSession(session.id)}
        >
          Share
        </button>
      )}
      <PeopleBar client={client} />
      <AccountButton client={client} />
      <WindowButtons />
    </header>
  );
}

/** The header over a Cloud workspace (S40): where it is and its status,
 * then search, the bell, Delete, the account and the window's buttons. */
function CloudWorkspaceHeader({ client, inert }: { client: Client; inert: boolean }) {
  const sidebar = useSidebar();
  return (
    <header class="ws-head" data-tauri-drag-region inert={inert} data-cloud-header>
      {sidebar.open && !sidebar.narrow && (
        <button class="ws-wordmark" style={{ width: `${sidebar.width}px` }} aria-label="Arugula Control: session overview" title="This machine’s session overview"
          disabled={!client.state} onClick={() => openOverview()}>
          Arugula <span aria-hidden="true">/</span> <strong>Control</strong>
        </button>
      )}
      <nav class="ws-crumbs" aria-label="Where you are" data-tauri-drag-region>
        <CloudWorkspaceCrumbs />
      </nav>
      <div class="bar-fill" data-tauri-drag-region />
      {client.state && <CommandsButton client={client} search />}
      <AttentionBell client={client} />
      <RebuildCloudWorkspace place="header" />
      <DeleteCloudWorkspace />
      <AccountButton client={client} />
      <WindowButtons />
    </header>
  );
}
