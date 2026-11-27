// The workspace: on a computer, the rail, the session sidebar, the header,
// the tab strip and the footer around the panes (ui/shell/, M108–M113); on
// a phone, its own header (ui/phone). Then the tab's panes (ui/panes/), and
// the layers over them (menus, dialogs; Go to… is the frame's).

import { EmptyState } from "../../shared/ui/block-states";
import { PageGuard } from "../../shared/ui/guard";
import { AppBar } from "./app-bar";
import type { Place } from "./places";
import { useEffect, useLayoutEffect, useState } from "preact/hooks";
import type { Client } from "../client";
import type { Cell } from "./cells";
import { useSubscribe, usePhone } from "../../shared/ui/hooks";
import { MenuLayer, PromptLayer } from "../../shared/ui/menu";
import { KeyBar, PhoneHeader } from "./phone";
import { ThreadLayer } from "./threads";
import { InLabs } from "./in-labs";
import { agentsRoute } from "./agents-route";
import { landOldDocs } from "./docs-route";
import { SettingsPage } from "./settings-page";
import { openSettings, settingsRoute } from "./settings-route";
import { settingsPath } from "../../shared/machine-paths";
import { landOnPlatform, platformHere, platformOrigin } from "../platform-links";
import { HostPicker, NewMachineNote, setMachineItems } from "./hosts";
import { ControlRequests, ShareDialog } from "./people";
import { directory } from "../hosts";
import { onShownProject, shownProject } from "../project-view";
import { DeveloperLayer } from "./developer";
import { RulesLayer } from "./rules";
import { SharingLayer } from "./sharing";
import { AccessLayer } from "./access";
import { DocShareLayer } from "./doc-share";
import { ConversationsLayer, openYourWork, YourWork, yourWorkHome } from "./conversations";
import { PickerLayer } from "./picker";
import { InstallHint } from "./notify";
import { DeploysWaitingNote } from "./deploys-waiting";
import { GettingStartedLayer, gettingStartedFor } from "./welcome";
import { machineSettingsItems, machineShown, remoteInputItems } from "./commands";
import { ControlBanner } from "./control-state";
import { MovingNote } from "./moving-note";
import { LinkNote } from "./link-note";
import { MaintenanceNote } from "../../shared/ui/maintenance-note";
import { AgentsNudge } from "./agents-setup";
import { HintEmpty, HintsWatch, HintToast } from "./hints";
import { NvimWatch } from "../open-in-editor";
import { NvimBlockColors } from "../nvim-block";
import { WhatsNewLayer } from "./whats-new";
import { PhoneOfferLayer } from "./phone-offer";
import { useWorkspaceKeys } from "./keys";
import { useGoToSource } from "./goto-source";
import { connection, ContextHeader } from "./shell/context-header";
import type { Renaming } from "./shell/rename-input";
import { StatusPill } from "./shell/status-pill";
import { Footer } from "../../shared/kit/frame/footer";
import { ActivityToggle } from "./shell/activity";
import { agentRunning } from "../../shared/needs-you";
import { Rail } from "./shell/rail";
import { noteLastTab, SessionSidebar, showSession } from "./shell/session-sidebar";
import { TabStrip } from "./shell/tab-strip";
import { PaneToolbar } from "./shell/pane-toolbar";
import { ActivityDrawer } from "./shell/activity";
import { AttentionDrawer } from "./shell/attention-drawer";
import { TabArea } from "./panes/tab-area";
import { TabAsks } from "./term-ask";
import { DragGhost } from "./panes/drop-overlay";
import { useHoverToSwitchTabs } from "./panes/drop";
import { currentPlace, landOldOverview } from "./places";
import { shellTitle } from "./titles";

/** Whose frame the workspace is drawn in (Arugula #989): its own, with the
 * shell's rail (the daemon's page, control's), or the app's, whose rail
 * holds the places instead. */
export type Frame = "own" | "app";

export function App({ client, cell, frame = "own" }: { client: Client; cell: Cell; frame?: Frame }) {
  useSubscribe((fn) => client.subscribe(fn));
  const phone = usePhone();
  const [renaming, setRenaming] = useState<Renaming>(null);

  useEffect(() => {
    // Focusing the window doesn't take the size from another window
    // (#333): typing here, showing a tab or "use this size" does.
    const visible = () => {
      if (document.visibilityState === "visible") client.wake();
    };
    document.addEventListener("visibilitychange", visible);
    return () => document.removeEventListener("visibilitychange", visible);
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
  useReportFocus(client, phone);
  useWorkspaceKeys(client, phone);
  useGoToSource(client, phone);
  gettingStartedFor(client);
  useTabTitle(client);
  // M113 (#1028): a computer draws the workspace once a machine is shown;
  // a phone keeps its own header.
  const ws = !phone && !!client.state;
  // The machine menu's "This machine" items are this client's: one
  // Settings… row to where they're a page, the app's Settings › Machines
  // wherever there's an app (control's page too: no dialogs there), or
  // `#settings` on a daemon's own page (Arugula #695); else (a daemon's own
  // page on a phone) the rows.
  useEffect(
    () =>
      setMachineItems(() => {
        const mine = machineSettingsItems(client);
        const page = mine.length > 0 && (ws || !!platformOrigin());
        const rows = page ? [{ label: "Settings…", run: () => openSettings("machine", machineShown(client)) }] : mine;
        // Arugula #1396: the owner's conversations, one click away once a pane is open.
        const work = client.state && !client.state.roles ? [{ label: "Your work…", run: () => openYourWork(client) }] : [];
        // Arugula #1194: whether people may type here through Arugula cloud.
        return [...work, ...remoteInputItems(client), ...rows];
      }),
    [client, ws],
  );
  // An old `#overview` link lands on the Machines page at this machine
  // (Arugula #1349), once the page knows which machine it shows; an old
  // `#docs`, on the app's Docs (Lift 6b).
  useSubscribe((fn) => directory.subscribe(fn));
  const known = !!directory.current || !!client.state;
  useEffect(() => {
    if (!known) return;
    const land = () => (landOldOverview(client, (s) => showSession(client, s)), landOldDocs());
    land();
    window.addEventListener("hashchange", land);
    return () => window.removeEventListener("hashchange", land);
  }, [client, known]);
  useLayoutEffect(() => {
    if (!ws) return;
    // The swarm, agents and Docs pages sit beside its rail (style.css).
    document.documentElement.dataset.workspace = "new";
    return () => void delete document.documentElement.dataset.workspace;
  }, [ws]);

  const state = client.state;
  const tab = client.tabView();
  const main = (
    <main class="main">
      {/* Its own guard (Arugula #615): the panes failing leaves the pages and the rail. */}
      <PageGuard at={`${client.session}:${client.tab}`}>
        {!state ? (
          <HostPicker />
        ) : client.shownSessions().length === 0 ? (
          <NoSessions client={client} phone={phone} />
        ) : tab ? (
          <TabArea client={client} tab={tab} cell={cell} phone={phone} />
        ) : null}
      </PageGuard>
    </main>
  );
  const layers = (
    <>
      <MenuLayer />
      <PromptLayer />
      <ThreadLayer phone={phone} client={client} />
      {state && <AgentsPageLayer client={client} />}
      <SettingsPageLayer client={client} />
      <ConversationsLayer />
      <InLabs on={client.flag("studio")}>{(l) => <l.AppsLayer />}</InLabs>
      <InLabs on={client.flag("sandbox-providers")}>{(l) => <l.SandboxesLayer />}</InLabs>
      <RulesLayer />
      <DocShareLayer />
      <DeveloperLayer />
      <SharingLayer />
      <AccessLayer />
      <PickerLayer />
      <GettingStartedLayer />
      <DragGhost />
      {/* An ask to drive shows over the share dialog (#551). */}
      <ShareDialog client={client} />
      <ControlRequests client={client} />
      {state && <HintsWatch client={client} />}
      {state && <NvimWatch client={client} />}
      {state && <NvimBlockColors client={client} />}
      {state && <HintToast client={client} />}
      <PhoneOfferLayer />
      <WhatsNewLayer client={client} />
    </>
  );
  if (ws) {
    noteLastTab(client);
    const session = client.shownSessions().find((s) => s.id === client.session);
    // The chrome is all outside `.main`, so the panes' cells and who sizes
    // the tab are the panes' own business. `frame` is the head and chrome
    // the platform page shares (shared/frame.css).
    // M113: while the link is down, what's shown is the last word from the
    // machine; the chrome greys what it can't vouch for (style.css).
    const stale = !!state && !client.connected;
    return (
      <div class={stale ? "app ws frame stale" : "app ws frame"}>
        {frame === "own" && <Rail client={client} />}
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
              {tab && <PaneToolbar client={client} tab={tab} />}
              {tab && <TabAsks client={client} tab={tab} phone={phone} />}
              {main}
              {session && <ActivityDrawer key={session.id} client={client} session={session.id} />}
            </div>
            {/* Under the header and beside the rail, as the panes are (Arugula #796). */}
            {/* M111: between the header and the footer, over the panes. */}
            <AttentionDrawer client={client} />
          </div>
          {state && <WorkFooter client={client} />}
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
      {/* #468: a deploy waiting for you, one tap away. */}
      {phone && <DeploysWaitingNote />}
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
      <SettingsPageLayer client={client} />
      <ConversationsLayer />
      <InLabs on={client.flag("studio")}>{(l) => <l.AppsLayer />}</InLabs>
      <InLabs on={client.flag("sandbox-providers")}>{(l) => <l.SandboxesLayer />}</InLabs>
      <RulesLayer />
      <DocShareLayer />
      <DeveloperLayer />
      <SharingLayer />
      <AccessLayer />
      <PickerLayer />
      <GettingStartedLayer />
      <DragGhost />
      {/* An ask to drive shows over the share dialog (#551). */}
      <ShareDialog client={client} />
      <ControlRequests client={client} />
      {state && <HintsWatch client={client} />}
      {state && <NvimWatch client={client} />}
      {state && <NvimBlockColors client={client} />}
      {state && <HintToast client={client} />}
      <PhoneOfferLayer />
      <WhatsNewLayer client={client} />
      <StatusPill client={client} />
    </div>
  );
}

/** The empty state, beside the sessions (or under the phone's header):
 * the owner's is Your work (Arugula #1396), their conversations on this
 * machine. Through control it says which machine this is and where to
 * switch, since the machine may not be the one meant. A guest can't make
 * one. */
function NoSessions({ client, phone }: { client: Client; phone: boolean }) {
  useSubscribe((fn) => directory.subscribe(fn));
  useSubscribe(onShownProject);
  const host = directory.current || "this machine";
  const guest = !!client.state?.roles;
  // The app's machine page shows one project's sessions (Arugula #1164).
  const project = shownProject();
  if (project)
    return (
      <div class="empty" data-no-sessions data-project-empty>
        <EmptyState
          where="page"
          title={guest ? `Nothing in ${project.name} on ${host}.` : `No ${project.name} sessions on ${host} yet.`}
          action={
            guest ? (
              project.machines && (
                <button class="secondary" data-project-empty-machines onClick={project.machines}>
                  Machines
                </button>
              )
            ) : (
              <span class="empty-new-session">
                <button class="primary" disabled={!client.connected} data-project-empty-new onClick={() => client.intent({ op: "new_session", name: null, from_pane: null })}>
                  New session
                </button>
                <span class="dim" data-project-empty-where>
                  On {project.name}
                </span>
              </span>
            )
          }
        />
      </div>
    );
  if (!guest && yourWorkHome())
    return (
      <div class="your-work-home" data-no-sessions data-your-work-home>
        <YourWork client={client} phone={phone} />
      </div>
    );
  return (
    <div class="empty" data-no-sessions>
      <EmptyState
        where="page"
        title={guest ? `Nothing on ${host} is shared with you yet.` : `No sessions on ${host}.`}
        why={
          guest
            ? "When its owner shares a session with you, it shows here."
            : "A session holds tabs, and each tab its panes: shells, agents, editors."
        }
        action={
          !guest && (
            <button class="primary" disabled={!client.connected} onClick={() => client.intent({ op: "new_session", name: null, from_pane: null })}>
              New session
            </button>
          )
        }
      >
        {directory.control && (
          <p class="empty-state-why" data-no-sessions-host>
            Not the machine you meant? Switch from the {phone ? "menu at the top" : "machine's name at the top left"}.
          </p>
        )}
      </EmptyState>
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
  const session = state ? client.shownSessions().find((s) => s.id === client.session)?.name : null;
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
  return (
    <PlaceGuard client={client} at="agents">
      <InLabs on={client.flag("agent-delegation")}>{(l) => <l.AgentsPage client={client} />}</InLabs>
    </PlaceGuard>
  );
}

/** The Settings page (Arugula #695), while its route is up. Where the
 * app's Settings is (control's page, the app), a section that's there now
 * lands on it (Arugula #1110), a machine's own once the machine shown is
 * known; control's page keeps Phones and browsers. */
function SettingsPageLayer({ client }: { client: Client }) {
  const [, setTick] = useState(0);
  useEffect(() => {
    const fn = () => setTick((t) => t + 1);
    window.addEventListener("hashchange", fn);
    return () => window.removeEventListener("hashchange", fn);
  }, []);
  useSubscribe((fn) => directory.subscribe(fn));
  const route = settingsRoute();
  const to = route ? movedTo(client, route.section) : null;
  useEffect(() => {
    if (to && to !== "wait") landOnPlatform(to);
  }, [to]);
  return route && !to ? (
    <PlaceGuard client={client} at="settings">
      <SettingsPage client={client} />
    </PlaceGuard>
  ) : null;
}

/** Where `#settings=<section>` is in the app's Settings now, "wait" while
 * the machine it's about isn't known yet, or null where it's drawn here: a
 * daemon's own page, and control's Phones and browsers. In the app, that's
 * You's Notifications, whose row leads back to control's. */
function movedTo(client: Client, section?: string): string | "wait" | null {
  if (!platformOrigin()) return null;
  const machine = machineShown(client);
  if ((section === "machine" || section === "remote-sessions") && !machine && !client.state) return "wait";
  return settingsPath(section, machine) ?? (platformHere() ? "/settings/you/notifications" : null);
}

/** A page over the panes in its guard (Arugula #615): one that throws while
 * drawing says so where it would have been, under the bar (the way out on a
 * phone), and the panes, the rail and the other pages go on working. In the
 * workspace, the Docs page sits beside the rail with no bar of its own. */
function PlaceGuard({ client, at, inWorkspace, children }: { client: Client; at: Place; inWorkspace?: boolean; children: preact.ComponentChildren }) {
  return (
    <PageGuard
      at={location.hash}
      frame={(note) => (
        <div class={inWorkspace ? "chat agents-page ws-page" : "chat agents-page"} data-page-failed-at={at}>
          {!inWorkspace && <AppBar client={client} at={at} />}
          {note}
        </div>
      )}
    >
      {children}
    </PageGuard>
  );
}

/** The places' names, for the footer. */
const PLACE_NAME: Record<Place, string> = { panes: "Machines", agents: "Agents", settings: "Settings" };

/** The frame's footer: the place, its keys, the agents running on this
 * machine (not while the link is down: they may not be, M113; a stopped
 * agent isn't, UI audit #154) and the session's Activity. */
function WorkFooter({ client }: { client: Client }) {
  const running = client.connected ? (client.state?.panes ?? []).filter(agentRunning).length : 0;
  const activity = client.session !== null && client.state?.sessions.length ? <ActivityToggle client={client} /> : null;
  return <Footer place={PLACE_NAME[currentPlace()]} running={running} extra={activity} />;
}
