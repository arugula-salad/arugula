// The workspace: on a computer, the rail, the session sidebar, the header,
// the tab strip and the footer around the panes (ui/shell/, M108–M113); on
// a phone, its own header (ui/phone). Then the tab's panes (ui/panes/), and
// the layers over them (menus, dialogs, the palette).

import { cloudPanes, cloudWorkspaceRoute, useCloudPanes } from "../cloud";
import { CloudTabs, RunDetails } from "./cloud";
import { CloudWorkspaceCrumbs, CloudWorkspaceMain, CloudWorkspaceTabStrip, DeleteCloudWorkspace } from "./cloud-workspaces";
import { RebuildCloudWorkspace } from "./cloud-workspace-rebuild";
import { useEffect, useLayoutEffect, useState } from "preact/hooks";
import type { Client } from "../client";
import type { Cell } from "./cells";
import { useSubscribe, usePhone } from "./hooks";
import { MenuLayer, PromptLayer } from "./menu";
import { KeyBar, PhoneHeader } from "./phone";
import { ThreadLayer } from "./threads";
import { InLabs } from "./in-labs";
import { agentsRoute } from "./agents-route";
import { useChatOpen } from "./places";
import { AppBar } from "./app-bar";
import { HostPicker, NewMachineNote, setMachineItems } from "./hosts";
import { ControlRequests, ShareDialog } from "./people";
import { directory } from "../hosts";
import { DeveloperLayer } from "./developer";
import { RulesLayer } from "./rules";
import { startAgent } from "./agent-dialog";
import { ConversationsLayer } from "./conversations";
import { PickerLayer } from "./picker";
import { InstallHint } from "./notify";
import { GettingStartedLayer, useFirstRun } from "./welcome";
import { machineSettingsItems } from "./commands";
import { PaletteLayer } from "./palette";
import { ControlBanner } from "./control-state";
import { AgentsNudge } from "./agents-setup";
import { useWorkspaceKeys } from "./keys";
import { connection, ContextHeader } from "./shell/context-header";
import type { Renaming } from "./shell/rename-input";
import { StatusFooter, StatusPill } from "./shell/status-footer";
import { Rail } from "./shell/rail";
import { noteLastTab, SessionSidebar } from "./shell/session-sidebar";
import { TabStrip } from "./shell/tab-strip";
import { PaneToolbar } from "./shell/pane-toolbar";
import { ActivityDrawer } from "./shell/activity";
import { AttentionDrawer } from "./shell/attention-drawer";
import { SessionOverview, useOverview } from "./shell/overview";
import { TabArea } from "./panes/tab-area";
import { DragGhost } from "./panes/drop-overlay";
import { useHoverToSwitchTabs } from "./panes/drop";

/** How long a hidden page keeps its connection to a sandbox host. */
const HIDDEN_GRACE_MS = 10_000;

export function App({ client, cell }: { client: Client; cell: Cell }) {
  useSubscribe((fn) => client.subscribe(fn));
  const phone = usePhone();
  const cloud = useCloudPanes();
  // S40: a Cloud workspace (or the list of them) is shown instead of a
  // machine's session; it needs no machine.
  const cloudWs = cloudWorkspaceRoute();
  const [renaming, setRenaming] = useState<Renaming>(null);

  useEffect(() => {
    // Focusing the window doesn't take the size from another window
    // (#333): typing here, showing a tab or "use this size" does.
    // A sandbox host sleeps when nothing holds it awake, and an open
    // connection does: let go of it while the page is hidden.
    let hidden: number | undefined;
    const visible = () => {
      clearTimeout(hidden);
      if (document.visibilityState === "visible") client.wake();
      else if (directory.sleeps) hidden = window.setTimeout(() => client.sleep(), HIDDEN_GRACE_MS);
    };
    document.addEventListener("visibilitychange", visible);
    return () => {
      clearTimeout(hidden);
      document.removeEventListener("visibilitychange", visible);
    };
  }, [client]);

  // Following someone stops as soon as you do something yourself.
  useEffect(() => {
    const stop = (e: Event) => {
      if (client.following !== null && !(e.target as HTMLElement).closest?.(".avatar")) client.follow(null);
    };
    window.addEventListener("pointerdown", stop, true);
    window.addEventListener("keydown", stop, true);
    return () => {
      window.removeEventListener("pointerdown", stop, true);
      window.removeEventListener("keydown", stop, true);
    };
  }, [client]);

  useHoverToSwitchTabs(client);
  // The machine menu's "This machine" items are this client's.
  useEffect(() => setMachineItems(() => machineSettingsItems(client)), [client]);
  useReportFocus(client, phone);
  useWorkspaceKeys(client, phone);
  useFirstRun(client);
  // M113 (#1028): a computer draws the workspace once a machine is shown;
  // a phone keeps its own header.
  const ws = !phone && (!!client.state || !!cloudWs);
  // M111: `#overview` lies over the tabs and panes (not on a phone).
  const overview = useOverview();
  useLayoutEffect(() => {
    if (!ws) return;
    // The swarm, chat and agents pages sit beside its rail (style.css).
    document.documentElement.dataset.workspace = "new";
    return () => void delete document.documentElement.dataset.workspace;
  }, [ws]);

  const state = client.state;
  const tab = client.tabView();
  // M73: the chat page covers the panes, which stay laid out under it (so
  // their terminals keep their size) but can't be reached.
  const chatOpen = useChatOpen() && !!state && client.hasThreads();
  const overviewOpen = ws && !!overview && !!state && !cloud && !cloudWs;
  const main = (
    <main class="main" inert={chatOpen || overviewOpen}>
      {cloudWs ? <CloudWorkspaceMain /> : cloud ? <RunDetails key={cloud} id={cloud} /> : !state ? (
        <HostPicker />
      ) : state.sessions.length === 0 ? (
        <NoSessions client={client} phone={phone} />
      ) : tab ? (
        <TabArea client={client} tab={tab} cell={cell} phone={phone} />
      ) : null}
    </main>
  );
  const layers = (
    <>
      <MenuLayer />
      <PromptLayer />
      <ThreadLayer phone={phone} />
      {state && <InLabs on={client.hasThreads()}>{(l) => <l.ChatPage client={client} />}</InLabs>}
      <InLabs on={client.hasThreads()}>{(l) => <l.LinkLayer />}</InLabs>
      {state && <AgentsPageLayer client={client} />}
      <ConversationsLayer />
      <InLabs on={client.flag("studio")}>{(l) => <l.AppsLayer />}</InLabs>
      <InLabs on={client.flag("vms")}>{(l) => <l.SandboxesLayer />}</InLabs>
      <RulesLayer />
      <DeveloperLayer />
      <PickerLayer />
      <PaletteLayer />
      <GettingStartedLayer />
      <DragGhost />
      {/* An ask to drive shows over the share dialog (#551). */}
      <ShareDialog client={client} />
      <ControlRequests client={client} />
    </>
  );
  if (ws) {
    noteLastTab(client);
    const session = state?.sessions.find((s) => s.id === client.session) ?? state?.sessions[0];
    // The chrome is all outside `.main`, so the panes' cells and who sizes
    // the tab are the panes' own business.
    // M113: while the link is down, what's shown is the last word from the
    // machine; the chrome greys what it can't vouch for (style.css).
    const stale = !!state && !client.connected && !cloudWs;
    return (
      <div class={stale ? "app ws stale" : "app ws"}>
        <Rail client={client} />
        <div class="ws-shell-body">
          <ContextHeader client={client} setRenaming={setRenaming} inert={chatOpen} />
          <div class="ws-row" inert={chatOpen}>
            <SessionSidebar client={client} renaming={renaming} setRenaming={setRenaming} />
            <div class="ws-work">
              {cloudWs?.workspace ? (
                <div class="ws-tabs">
                  <CloudWorkspaceTabStrip />
                </div>
              ) : cloudWs ? null : session ? (
                <div class="ws-tabs">
                  <TabStrip client={client} session={session} renaming={renaming} setRenaming={setRenaming} />
                </div>
              ) : (
                cloudPanes.opened.length > 0 && (
                  <div class="ws-tabs">
                    <div class="tabbar" role="tablist">
                      <CloudTabs />
                    </div>
                  </div>
                )
              )}
              {stale && (
                <div class="ws-stale" role="status" data-stale-note>
                  {connection(client).why}. What's shown is how it was; nothing here is live until it's back.
                </div>
              )}
              <ControlBanner client={client} />
              <NewMachineNote />
              <AgentsNudge client={client} />
              {tab && !cloud && !cloudWs && !overviewOpen && <PaneToolbar client={client} tab={tab} />}
              {main}
              {session && !cloud && !cloudWs && <ActivityDrawer key={session.id} client={client} session={session.id} />}
              {overviewOpen && <SessionOverview client={client} session={overview?.session} />}
            </div>
            {/* M111: between the header and the footer, over the panes. */}
            <AttentionDrawer client={client} />
          </div>
          {state && <StatusFooter client={client} />}
        </div>
        {layers}
        {!cloud && !cloudWs && <StatusPill client={client} errorsOnly />}
      </div>
    );
  }
  return (
    <div class={phone ? "app phone" : "app"}>
      {/* A computer gets here only before a machine answers (or with a
          cloud run's details and none shown). */}
      {state && phone && <PhoneHeader client={client} />}
      {!state && cloud && <AppBar client={client} at="panes" all>{!phone && <div class="tabbar" role="tablist"><CloudTabs /><button onClick={() => startAgent(client, {})}>New agent</button></div>}</AppBar>}
      {phone && cloudWs && <nav class="cloud-ws-phone-head" aria-label="Where you are"><CloudWorkspaceCrumbs /><span class="bar-fill" /><RebuildCloudWorkspace place="header" /><DeleteCloudWorkspace /></nav>}
      {phone && cloudWs?.workspace && <div class="cloud-phone-tabs">{state && <button onClick={() => cloudPanes.local()}>This machine</button>}<CloudWorkspaceTabStrip /></div>}
      {phone && !cloudWs && cloudPanes.opened.length > 0 && <div class="tabbar cloud-phone-tabs" role="tablist">{state && <button onClick={() => cloudPanes.local()}>This machine</button>}<CloudTabs /></div>}
      {state && <ControlBanner client={client} />}
      {state && <NewMachineNote />}
      {state && <AgentsNudge client={client} />}
      {main}
      {phone && state && state.sessions.length > 0 && client.panes.has(client.active() ?? -1) && <KeyBar client={client} />}
      <MenuLayer />
      <PromptLayer />
      <ThreadLayer phone={phone} />
      {state && <InLabs on={client.hasThreads()}>{(l) => <l.ChatPage client={client} />}</InLabs>}
      <InLabs on={client.hasThreads()}>{(l) => <l.LinkLayer />}</InLabs>
      {state && <AgentsPageLayer client={client} />}
      <ConversationsLayer />
      <InLabs on={client.flag("studio")}>{(l) => <l.AppsLayer />}</InLabs>
      <InLabs on={client.flag("vms")}>{(l) => <l.SandboxesLayer />}</InLabs>
      <RulesLayer />
      <DeveloperLayer />
      <PickerLayer />
      <PaletteLayer />
      <GettingStartedLayer />
      {phone && state && <InstallHint />}
      <DragGhost />
      {/* An ask to drive shows over the share dialog (#551). */}
      <ShareDialog client={client} />
      <ControlRequests client={client} />
      {!cloud && !cloudWs && <StatusPill client={client} />}
    </div>
  );
}

/** The empty state, beside the sessions (or under the phone's header).
 * Through control it says which machine this is and where to switch,
 * since the machine may not be the one meant. A guest can't make one. */
function NoSessions({ client, phone }: { client: Client; phone: boolean }) {
  useSubscribe((fn) => directory.subscribe(fn));
  const host = directory.current || "this machine";
  const guest = !!client.state?.roles;
  return (
    <div class="empty" data-no-sessions>
      <p class="empty-title">{guest ? `Nothing on ${host} is shared with you yet.` : `No sessions on ${host}.`}</p>
      <p class="dim">
        {guest
          ? "When its owner shares a session with you, it shows here."
          : "A session holds tabs, and each tab its panes: shells, agents, editors."}
      </p>
      {directory.control && (
        <p data-no-sessions-host>Not the machine you meant? Switch from the {phone ? "menu at the top" : "machine's name at the top left"}.</p>
      )}
      {!guest && (
        <button class="primary" disabled={!client.connected} onClick={() => client.intent({ op: "new_session", name: null, from_pane: null })}>
          New session
        </button>
      )}
    </div>
  );
}

// ---------------------------------------------------------------- attention

/** Tell the daemon which pane this window is looking at, so it doesn't
 * notify you about the pane in front of you. */
function useReportFocus(client: Client, phone: boolean) {
  const active = client.active();
  useEffect(() => {
    const report = () => {
      const looking = document.visibilityState === "visible" && (phone || document.hasFocus());
      client.focusPane(looking ? (client.active() ?? null) : null);
    };
    report();
    window.addEventListener("focus", report);
    window.addEventListener("blur", report);
    document.addEventListener("visibilitychange", report);
    return () => {
      window.removeEventListener("focus", report);
      window.removeEventListener("blur", report);
      document.removeEventListener("visibilitychange", report);
    };
  }, [client, active, phone, client.connected]);
}

/** The Agents page (#403): loads Labs only while its route is up. */
function AgentsPageLayer({ client }: { client: Client }) {
  const [, setTick] = useState(0);
  useEffect(() => {
    const fn = () => setTick((t) => t + 1);
    window.addEventListener("hashchange", fn);
    return () => window.removeEventListener("hashchange", fn);
  }, []);
  if (!agentsRoute()) return null;
  return <InLabs on={client.flag("agents")}>{(l) => <l.AgentsPage client={client} />}</InLabs>;
}
