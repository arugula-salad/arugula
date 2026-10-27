// The workspace: on a computer, the rail, the session sidebar, the header,
// the tab strip and the footer around the panes (ui/shell/, M108–M113); on
// a phone, its own header (ui/phone). Then the tab's panes (ui/panes/), and
// the layers over them (menus, dialogs, the palette).

import { useEffect, useLayoutEffect, useState } from "preact/hooks";
import type { Client } from "../client";
import type { Cell } from "./cells";
import { useSubscribe, usePhone } from "../../shared/ui/hooks";
import { MenuLayer, PromptLayer } from "../../shared/ui/menu";
import { KeyBar, PhoneHeader } from "./phone";
import { ThreadLayer } from "./threads";
import { InLabs } from "./in-labs";
import { agentsRoute } from "./agents-route";
import { docsRoute } from "./docs-route";
import { HostPicker, NewMachineNote, setMachineItems } from "./hosts";
import { ControlRequests, ShareDialog } from "./people";
import { directory } from "../hosts";
import { DeveloperLayer } from "./developer";
import { RulesLayer } from "./rules";
import { SharingLayer } from "./sharing";
import { DocShareLayer } from "./doc-share";
import { ConversationsLayer } from "./conversations";
import { PickerLayer } from "./picker";
import { InstallHint } from "./notify";
import { GettingStartedLayer, useFirstRun } from "./welcome";
import { machineSettingsItems } from "./commands";
import { PaletteLayer } from "./palette";
import { ControlBanner } from "./control-state";
import { MovingNote } from "./moving-note";
import { LinkNote } from "./link-note";
import { MaintenanceNote } from "../../shared/ui/maintenance-note";
import { AgentsNudge } from "./agents-setup";
import { HintEmpty, HintsWatch, HintToast } from "./hints";
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
import { currentPlace } from "./places";
import { shellTitle } from "./titles";

/** How long a hidden page keeps its connection to a sandbox host. */
const HIDDEN_GRACE_MS = 10_000;

export function App({ client, cell }: { client: Client; cell: Cell }) {
  useSubscribe((fn) => client.subscribe(fn));
  const phone = usePhone();
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
      if ((client.following !== null || client.watching !== null) && !(e.target as HTMLElement).closest?.(".avatar")) client.follow(null);
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
  useTabTitle(client);
  // M113 (#1028): a computer draws the workspace once a machine is shown;
  // a phone keeps its own header.
  const ws = !phone && !!client.state;
  // M111: `#overview` lies over the tabs and panes (not on a phone).
  const overview = useOverview();
  useLayoutEffect(() => {
    if (!ws) return;
    // The swarm, agents and Docs pages sit beside its rail (style.css).
    document.documentElement.dataset.workspace = "new";
    return () => void delete document.documentElement.dataset.workspace;
  }, [ws]);

  const state = client.state;
  const tab = client.tabView();
  // Chat is on the platform page (M153, M155).
  const overviewOpen = ws && !!overview && !!state;
  const main = (
    <main class="main" inert={overviewOpen}>
      {!state ? (
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
      <ThreadLayer phone={phone} client={client} />
      {state && <AgentsPageLayer client={client} />}
      {state && <DocsPageLayer client={client} />}
      <ConversationsLayer />
      <InLabs on={client.flag("studio")}>{(l) => <l.AppsLayer />}</InLabs>
      <InLabs on={client.flag("sandbox-providers")}>{(l) => <l.SandboxesLayer />}</InLabs>
      <RulesLayer />
      <DocShareLayer />
      <DeveloperLayer />
      <SharingLayer />
      <PickerLayer />
      <PaletteLayer />
      <GettingStartedLayer />
      <DragGhost />
      {/* An ask to drive shows over the share dialog (#551). */}
      <ShareDialog client={client} />
      <ControlRequests client={client} />
      {state && <HintsWatch client={client} />}
      {state && <HintToast client={client} />}
    </>
  );
  if (ws) {
    noteLastTab(client);
    const session = state?.sessions.find((s) => s.id === client.session) ?? state?.sessions[0];
    // The chrome is all outside `.main`, so the panes' cells and who sizes
    // the tab are the panes' own business. `frame` is the head and chrome
    // the platform page shares (shared/frame.css).
    // M113: while the link is down, what's shown is the last word from the
    // machine; the chrome greys what it can't vouch for (style.css).
    const stale = !!state && !client.connected;
    return (
      <div class={stale ? "app ws frame stale" : "app ws frame"}>
        <Rail client={client} />
        <div class="ws-shell-body">
          <ContextHeader client={client} setRenaming={setRenaming} />
          <div class="ws-row">
            <SessionSidebar client={client} renaming={renaming} setRenaming={setRenaming} />
            <div class="ws-work">
              {session && (
                <div class="ws-tabs">
                  <TabStrip client={client} session={session} renaming={renaming} setRenaming={setRenaming} />
                </div>
              )}
              {stale && (
                <div class="ws-stale" role="status" data-stale-note>
                  {connection(client).why}. What's shown is how it was; nothing here is live until it's back.
                </div>
              )}
              <ControlBanner client={client} />
              <LinkNote />
              <MovingNote />
              <MaintenanceNote />
              <NewMachineNote />
              <AgentsNudge client={client} />
              {tab && !overviewOpen && <PaneToolbar client={client} tab={tab} />}
              {main}
              {session && <ActivityDrawer key={session.id} client={client} session={session.id} />}
              {overviewOpen && <SessionOverview client={client} session={overview?.session} />}
            </div>
            {/* M111: between the header and the footer, over the panes. */}
            <AttentionDrawer client={client} />
          </div>
          {state && <StatusFooter client={client} />}
        </div>
        {layers}
        <StatusPill client={client} errorsOnly />
      </div>
    );
  }
  return (
    <div class={phone ? "app phone" : "app"}>
      {/* A computer gets here only before a machine answers. */}
      {state && phone && <PhoneHeader client={client} />}
      {state && <ControlBanner client={client} />}
      <LinkNote />
      <MovingNote />
      <MaintenanceNote />
      {state && <NewMachineNote />}
      {state && <AgentsNudge client={client} />}
      {main}
      {/* In the column, above the key bar (UI audit #199). */}
      {phone && state && <InstallHint />}
      {/* Only where this person's typing reaches the pane: not a viewer's,
          nor a read-only link's (UI audit #149). */}
      {phone && state && state.sessions.length > 0 && client.panes.has(client.active() ?? -1) && client.mayType(client.active()!) && <KeyBar client={client} />}
      <MenuLayer />
      <PromptLayer />
      <ThreadLayer phone={phone} client={client} />
      {state && <AgentsPageLayer client={client} />}
      {state && <DocsPageLayer client={client} />}
      <ConversationsLayer />
      <InLabs on={client.flag("studio")}>{(l) => <l.AppsLayer />}</InLabs>
      <InLabs on={client.flag("sandbox-providers")}>{(l) => <l.SandboxesLayer />}</InLabs>
      <RulesLayer />
      <DocShareLayer />
      <DeveloperLayer />
      <SharingLayer />
      <PickerLayer />
      <PaletteLayer />
      <GettingStartedLayer />
      <DragGhost />
      {/* An ask to drive shows over the share dialog (#551). */}
      <ShareDialog client={client} />
      <ControlRequests client={client} />
      {state && <HintsWatch client={client} />}
      {state && <HintToast client={client} />}
      <StatusPill client={client} />
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
      <HintEmpty client={client} />
    </div>
  );
}

// ---------------------------------------------------------------- attention

/** The tab's title says what's shown (UI audit #170): the session and the
 * machine, or the place. */
function useTabTitle(client: Client) {
  useSubscribe((fn) => directory.subscribe(fn));
  const [, setTick] = useState(0);
  useEffect(() => {
    const fn = () => setTick((t) => t + 1);
    window.addEventListener("hashchange", fn);
    return () => window.removeEventListener("hashchange", fn);
  }, []);
  const state = client.state;
  const session = state ? (state.sessions.find((s) => s.id === client.session) ?? state.sessions[0])?.name : null;
  const title = shellTitle(currentPlace(), directory.current || null, session);
  useEffect(() => void (document.title = title), [title]);
}

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
  return <InLabs on={client.flag("agent-delegation")}>{(l) => <l.AgentsPage client={client} />}</InLabs>;
}

/** The Docs page (#1494): a chunk of its own, fetched while its route is
 * up. Not Labs' since native #204. */
function DocsPageLayer({ client }: { client: Client }) {
  const [, setTick] = useState(0);
  const [page, setPage] = useState<typeof import("./docs-page") | null>(null);
  useEffect(() => {
    const fn = () => setTick((t) => t + 1);
    window.addEventListener("hashchange", fn);
    return () => window.removeEventListener("hashchange", fn);
  }, []);
  const on = docsRoute();
  useEffect(() => {
    if (on && !page) import("./docs-page").then(setPage, (e) => console.error("The Docs page didn't load", e));
  }, [on]);
  if (!on || !page) return null;
  return <page.DocsPage client={client} />;
}
