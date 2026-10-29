// What the menus offer, in one place (#139): the pane menu, a tab's menu,
// the `+` button's menu and the session menu are built here, and the
// command palette reads the same lists, so the two can't drift.

import { hostMenuShown } from "./hosts";
import { opPath } from "../ops";
import { paneIds, type Client } from "../client";
import type { Machine, OpenRequest, PaneId, Policy, SessionId, TabView } from "../../shared/proto";
import { askText, openMenu, type MenuItem } from "../../shared/ui/menu";
import { openSandboxes, pickApp } from "../labs-load";
import { driveItems, getControlSession, shareSession } from "./people";
import { hasChat } from "./places";
import { newRemote, openChanges, openEditor, openIssue, openPort, openPortAt, openPr, openWorkspace, openPeople, pinBoard, pinChatRoom, openTeamChat, remoteHosts } from "../blocks";
import { startAgent } from "./agent-dialog";
import { pickConversation } from "./conversations";
import { openPicker } from "./picker";
import { openPalette } from "./palette";
import { agentNotifyItems, notificationItems, notifyThisDevice } from "./notify";
import { openGettingStarted } from "./welcome";
import { openDeveloper } from "./developer";
import { openMachineTelemetry } from "./machine-telemetry";
import { openSharing } from "./sharing";
import { openAgentGrants } from "./agent-grants";
import { directory } from "../hosts";
import { openRules } from "./rules";
import { openAppearance } from "../../shared/ui/appearance";
import { desktopApp, desktopPlatform, openInNewWindow } from "../../shared/desktop";
import { keyHint } from "./keys";
import { sessionThreadItems, threadItems } from "./threads";
import { docShareItems } from "./doc-share";
import { focusItems } from "./panes/layout";
import { watchingNote } from "./shell/live";
import { bigTipsOn, setBigTips, setTips, tipsOn } from "../../shared/seen";
import { clientTarget, EDITORS, editorShown, openInEditor, setEditorPref } from "../open-in-editor";
import { openWhatsNew } from "./whats-new";
import { openPanel } from "../trust/common";
import { addPlugin, NVIM_DOCS, openHelp, openHelpCard, runsNvim, useNvim } from "../hints/nvim";


/** A pane's right-click menu. `workspace` is the directory it's in when
 * that is a chant workspace (M34). */
export function paneItems(client: Client, id: PaneId, phone: boolean, workspace: string | null): MenuItem[] {
  const info = client.info(id);
  const entry = client.panes.get(id);
  // #17: a shell on another host, beside this pane.
  const elsewhere = (): MenuItem[] =>
    remoteHosts().map((h) => ({ label: `Split right on ${h}`, run: () => void newRemote(client, h, { split: id }) }));
  const moveToTab: MenuItem = {
    label: "Move to new tab",
    disabled: (client.tabOfPane(id) && paneIds(client.tabOfPane(id)!).length < 2) ?? true,
    run: () => client.intent({ op: "break_pane", pane: id, session: client.session!, index: null }),
  };
  const closePane: MenuItem = { label: "Close pane", danger: true, run: () => client.intent({ op: "close_pane", pane: id }) };
  // M110: fill the tab with it (everyone's view), or give the layout back.
  const focus = phone ? [] : focusItems(client, client.tabOfPane(id), id);
  const cwd = client.cwd(id);
  // UI audit #158: a viewer (a share's, a read-only link's) only watches.
  const watching = watchingNote(client, client.sessionOfPane(id));
  if (watching)
    return [
      { header: watching },
      ...focus,
      { label: "Copy working directory", disabled: !cwd, run: () => cwd && void navigator.clipboard?.writeText(cwd) },
      ...threadItems(client, id),
    ];

  if (info?.type === "remote") {
    // Its terminal is its host's: here, only where it is.
    const at = client.blocks.get(id)?.state as { host: string; pane: PaneId } | null;
    return [
      ...(at ? [{ header: `%${at.pane} on ${at.host}` } as MenuItem] : []),
      ...focus,
      { label: "Split right", run: () => client.intent({ op: "split", pane: id, edge: "right" }) },
      { label: "Split down", run: () => client.intent({ op: "split", pane: id, edge: "bottom" }) },
      ...elsewhere(),
      "separator",
      moveToTab,
      closePane,
    ];
  }

  const tabId = client.tabOfPane(id)?.id;
  const tabMachine = tabId === undefined ? undefined : client.tabMachine(tabId);
  const mine = client.machine(id);
  const own = mine !== undefined && "pane" in mine.owner && mine.owner.pane === id;
  const owner = !client.state?.roles;
  const view = info?.type === "doc" ? undefined : entry?.view;
  // What a terminal's own menu offers first, before anything about panes.
  const clipboard: MenuItem[] = view
    ? [
        { label: "Copy", shortcut: desktopPlatform() === "macos" ? undefined : "Ctrl+Shift+C", disabled: !view.selection(), run: () => void navigator.clipboard?.writeText(view.selection()) },
        ...(client.mayType(id)
          ? [
              {
                label: "Paste",
                shortcut: desktopPlatform() === "macos" ? undefined : "Ctrl+Shift+V",
                hint: "If the browser won't give us the clipboard, paste with the keyboard",
                run: () => {
                  void navigator.clipboard?.readText?.().then((t) => t && view.paste(t)).catch(() => {});
                  view.focus();
                },
              } as MenuItem,
            ]
          : []),
        { label: "Select all", run: () => view.selectAll() },
        { label: "Clear scrollback", run: () => view.clearScrollback() },
        "separator",
      ]
    : [];
  const sub = (label: string, items: MenuItem[]): MenuItem[] => {
    return items.some((i) => i !== "separator") ? [{ label, submenu: items }] : [];
  };
  const openBeside: MenuItem[] = [
    ...(client.has("vms") ? [{ label: "New VM pane on the right", run: () => void client.newVm({ split: id }) } as MenuItem] : []),
    ...elsewhere(),
    {
      label: "Open a web page…",
      run: async () => {
        const url = await askText("Open a web page", "", "https://… or example.com");
        if (url) void client.api("/api/blocks", { type: "browser", config: { url }, split: id } satisfies OpenRequest, "couldn't open that page");
      },
    },
    // A port where this pane runs: its machine, or this host.
    { label: mine ? "Open a port on this machine…" : "Open a port…", run: () => void openPort(client, { split: id, host: mine?.id, local: !mine }) },
    { label: "Start an agent…", run: () => startAgent(client, { split: id, from: id }) },
    { label: "Start a chat", run: () => void client.newChat({ split: id, from: id }) },
    // #123: the team's chat beside it, or one room of it, where chat is on.
    ...(!client.state?.roles && hasChat()
      ? [
          { label: "Team chat", run: () => void openTeamChat(client, { from: id }) } as MenuItem,
          { label: "Pin a chat room…", run: () => void pinChatRoom(client, { from: id }) } as MenuItem,
        ]
      : []),
    // Who's around in a team, live, on arugula.stream (the block says so
    // when control has no arugula.stream app, or chat is off).
    ...(!client.state?.roles && getControlSession()?.phase === "ready" ? [{ label: "People", run: () => void openPeople(client, { from: id }) } as MenuItem] : []),
    // A project's board beside it, with the boards flag.
    ...(!client.state?.roles && client.flag("boards") ? [{ label: "Pin a board…", run: () => void pinBoard(client, { from: id }) } as MenuItem] : []),
    // M33: Claude Code conversations from terminals and the desktop app.
    ...(owner ? [{ label: "Claude Code conversations…", run: () => pickConversation(client, { split: id, cwd: cwd ?? undefined }) } as MenuItem] : []),
    // M35: a studio app's box beside this pane.
    ...(owner && client.has("studio") ? [{ label: "Open a studio app…", run: () => pickApp(client, { split: id }) } as MenuItem] : []),
    // M36: a pull request beside it (N: in its repository).
    ...(owner ? [{ label: "Open pull request…", run: () => void openPr(client, { split: id, dir: cwd }) } as MenuItem] : []),
    // M37: an issue beside it (N: in its repository).
    ...(owner ? [{ label: "Open issue…", run: () => void openIssue(client, { split: id, dir: cwd }) } as MenuItem] : []),
    // M27: VS Code where this pane runs, in its directory, or (#1731) its
    // nvim, as the Editor preference says. The owner's, like ports.
    ...(entry && owner ? [{ label: "Open in editor", run: () => void openInEditor(clientTarget(client, id, () => openEditor(client, id))) } as MenuItem] : []),
    // M11: what changed in its repository, where it runs.
    ...(owner ? [{ id: "changes", label: "Changes", run: () => openChanges(client, id) } as MenuItem] : []),
    ...(workspace ? [{ label: "Open as workspace", run: () => openWorkspace(client, workspace, id) } as MenuItem] : []),
  ];
  const share: MenuItem[] = [
    // The owner's: the share form for this pane's session, opening here,
    // with their window following ours (#1473).
    ...(owner && client.sessionOfPane(id) !== null
      ? [{ label: "Share this pane…", run: () => shareSession(client.sessionOfPane(id)!, { pane: id }) } as MenuItem]
      : []),
    // A dial-out host's links would be on a daemon nobody can reach.
    ...(client.base.startsWith("/")
      ? []
      : [
          {
            label: "Share read-only link…",
            run: async () => {
              const url = await client.share(id);
              if (url) await askText("Read-only link to this pane, for an hour (copied)", url);
            },
          } as MenuItem,
        ]),
    // An ssh command for a guest with only OpenSSH. The owner's. Only where
    // they've turned on Guest SSH access, a sharing setting (native #205).
    ...(client.base.startsWith("/") || !owner || !client.guestSsh()
      ? []
      : [
          {
            label: "Invite over ssh…",
            run: async () => {
              const cmd = await client.guestInvite(id);
              if (cmd) await askText("Read-only ssh invite to this pane, one login, for an hour (copied)", cmd);
            },
          } as MenuItem,
        ]),
    ...(own && !tabMachine
      ? [{ label: "Share machine with tab", run: () => void client.api(`/api/panes/${id}/share-machine`) } as MenuItem]
      : []),
    ...driveItems(client, id),
  ];
  return [
    // #1066: a Doc's link, the owner's.
    ...(info?.type === "doc" ? [...docShareItems(client, id), "separator" as const] : []),
    ...clipboard,
    ...focus,
    { label: "Split right", run: () => client.intent({ op: "split", pane: id, edge: "right" }) },
    { label: "Split down", run: () => client.intent({ op: "split", pane: id, edge: "bottom" }) },
    ...(tabMachine ? [{ label: "Split (local)", run: () => client.intent({ op: "split", pane: id, edge: "right", local: true }) } as MenuItem] : []),
    ...sub("Open beside", openBeside),
    ...sub("Move", [
      moveToTab,
      { id: "picker", label: "Go to directory…", shortcut: keyHint("picker"), run: () => openPicker(client, id, phone) },
    ]),
    "separator",
    { label: "Copy working directory", disabled: !cwd, run: () => cwd && void navigator.clipboard?.writeText(cwd) },
    // M70: a file onto this pane's host, its path pasted in (for an agent
    // there to read).
    ...(entry && client.mayType(id)
      ? [{ label: "Attach file…", shortcut: desktopPlatform() === "macos" ? keyHint("desktop.attach") : undefined, run: () => void client.attachFiles(id) } as MenuItem]
      : []),
    // #1735: nvim's help card, over this pane, when nvim runs in it.
    ...(runsNvim(info) ? [{ id: "nvim-help", label: "nvim help", run: () => openHelpCard(client, id) } as MenuItem] : []),
    ...sub("Share", share),
    // M61: the people's conversation about this pane.
    ...sub("Thread", threadItems(client, id)),
    "separator",
    ...(info && (info.attention === "needs_input" || info.attention === "done")
      ? [{ label: "Dismiss", run: () => void client.act({ action: "dismiss", pane: id }) } as MenuItem]
      : []),
    ...sub("Pane settings", [
      ...restartItems(client, id),
      "separator",
      {
        label: "Shell integration (new shells)",
        checked: info?.integration ?? true,
        run: () => client.paneOp(id, { op: "set_integration", on: !(info?.integration ?? true) }),
      },
      {
        label: "Forget history",
        confirm: "Forget history? Click again to confirm",
        run: () => client.paneOp(id, { op: "purge" }),
      },
    ]),
    closePane,
  ];
}

/** A tab's right-click menu. */
export function tabItems(client: Client, tab: TabView, rename: () => void): MenuItem[] {
  const machine = client.tabMachine(tab.id);
  // UI audit #158: nothing a viewer could do to a tab.
  const watching = watchingNote(client, client.sessionOfTab(tab.id) ?? null);
  if (watching) return [{ header: watching }, ...(desktopApp() ? [{ label: "Open in new window", run: () => openInNewWindow(client.active(tab.id)) } as MenuItem] : [])];
  return [
    { label: "Rename tab", run: rename },
    { id: "new-tab", label: "New tab", run: () => client.intent({ op: "new_tab", session: client.session!, from_pane: client.active(tab.id) ?? null }) },
    ...(client.has("vms") ? [{ id: "new-vm-tab", label: "New VM tab", run: () => void client.newVm({ session: client.session!, tab: true }) } as MenuItem] : []),
    { id: "picker", label: "Go to directory…", shortcut: keyHint("picker"), run: () => openPicker(client, client.active(tab.id)) },
    // The desktop app (M46): this tab in a window of its own.
    ...(desktopApp() ? [{ label: "Open in new window", run: () => openInNewWindow(client.active(tab.id)) } as MenuItem] : []),
    // M11: what changed in the active pane's repository, on its machine.
    ...(!client.state?.roles && client.active(tab.id) !== undefined
      ? [{ id: "changes", label: "Changes", run: () => openChanges(client, client.active(tab.id)!) } as MenuItem]
      : []),
    ...machineItems(client, tab),
    "separator",
    { label: machine ? "Close tab and machine" : "Close tab", danger: true, run: () => client.intent({ op: "close_tab", tab: tab.id }) },
  ];
}

/** The `+` button's menu: a new tab, and blocks in tabs of their own. */
export function newTabItems(client: Client, session: SessionId): MenuItem[] {
  // Each one a verb, "New …" or "Open …" (UI audit #171).
  return [
    { id: "new-tab", label: "New tab", run: () => client.intent({ op: "new_tab", session, from_pane: client.active() ?? null }) },
    ...(client.has("vms") ? [{ id: "new-vm-tab", label: "New VM tab", run: () => void client.newVm({ session, tab: true }) } as MenuItem] : []),
    { label: "New tab in a directory…", shortcut: keyHint("picker"), disabled: client.active() === undefined, run: () => openPicker(client, client.active()) },
    // M35: a studio app's box, in a tab of its own. The owner's.
    ...(!client.state?.roles && client.has("studio") ? [{ label: "Open a studio app…", run: () => pickApp(client, { session }) } as MenuItem] : []),
    // #123: the team's chat, in a tab of its own.
    ...(!client.state?.roles && hasChat() ? [{ label: "Open team chat", run: () => void openTeamChat(client, { session }) } as MenuItem] : []),
    // M36: a pull request, in a tab of its own.
    ...(!client.state?.roles ? [{ label: "Open pull request…", run: () => void openPr(client, { session }) } as MenuItem] : []),
    // M37: an issue, in a tab of its own.
    ...(!client.state?.roles ? [{ label: "Open issue…", run: () => void openIssue(client, { session }) } as MenuItem] : []),
    // #17: a tab here whose shell runs on another host.
    ...remoteHosts().map((h): MenuItem => ({ label: `New tab on ${h}`, run: () => void newRemote(client, h, { session }) })),
  ];
}

/** The session's menu: the sidebar row's … and the header's ▾ (after its
 * list of sessions) show the same one (UI audit #171). What acts on this
 * session first, then a new one, then closing it; settings for the
 * device, the machine and the account are in the account menu
 * (`settingsItems`). */
export function sessionItems(client: Client, session: SessionId, rename: () => void): MenuItem[] {
  // A guest's: the session's thread and notifications. Making, renaming
  // and closing sessions are the machine owner's (M113).
  if (client.state?.roles) return [...sessionThreadItems(client, session), ...agentNotifyItems(client, session)];
  return [
    { label: "Rename session", run: rename },
    // M61: the people's conversation about this session.
    ...sessionThreadItems(client, session),
    // Sharing is the daemon's owner's (M13).
    { label: "Share session…", run: () => shareSession(session) },
    ...agentNotifyItems(client, session),
    "separator",
    { label: "New session", run: () => client.intent({ op: "new_session", name: null, from_pane: client.active() ?? null }) },
    // VMs and sandboxes need wisp (#180).
    ...(client.has("vms")
      ? [
          { id: "new-vm-tab", label: "New VM tab", run: () => void client.newVm({ session, tab: true }) } as MenuItem,
          { label: "Sandboxes…", run: () => openSandboxes(client) } as MenuItem,
        ]
      : []),
    "separator",
    { label: "Close session", danger: true, run: () => client.intent({ op: "close_session", session }) },
  ];
}

/** What the session's menu holds, in a few words, for the tooltips of the
 * buttons that open it: read from its items, so they never promise one it
 * hasn't (UI audit #171). */
export function sessionMenuHint(items: MenuItem[]): string {
  const has = (re: RegExp) => items.some((i) => typeof i === "object" && "label" in i && re.test(i.label));
  const words = [
    has(/^Rename/) && "rename",
    has(/thread/) && "its thread",
    has(/^Share/) && "share",
    has(/^Notify/) && "notifications",
    has(/^New session/) && "a new session",
    has(/^Close/) && "close",
  ].filter((w): w is string => !!w);
  return words.join(", ");
}

/** The account menu's settings: this device, this machine, and help. The
 * same everywhere (the panes, the swarm, chat, agents), with or without a
 * session. */
export function settingsItems(client: Client | null, opts: { machine?: boolean } = {}): MenuItem[] {
  // The machine's own settings are in the machine menu, where there is
  // one; else (a daemon's page with no other machine) here. The rail's
  // Settings always has them: its tooltip says so (UI audit #174).
  const machine = client && (opts.machine || !hostMenuShown()) ? machineSettingsItems(client) : [];
  return [
    ...(machine.length ? [{ header: "This machine" } as MenuItem, ...machine, "separator" as const] : []),
    // How the page looks is this device's, with or without a machine.
    { header: "This device" },
    ...(client?.state ? notificationItems(client) : []),
    { label: "Appearance…", run: openAppearance },
    // Hints from facts (shell/hints): the account's, through control.
    {
      label: "Tips",
      hint: "A one-time mention when something you can use turns up, like a port listening in a pane",
      checked: tipsOn(),
      run: () => setTips(!tipsOn()),
    },
    // A pane modal's "Don't show tips like this again" turns these off;
    // this says so, and turns them back on. Tips off hides them too.
    {
      label: "Big tips",
      hint: "Tips that cover a pane until you close them, like a feature's help card",
      checked: bigTipsOn(),
      run: () => setBigTips(!bigTipsOn()),
    },
    // #1731: what Open in editor opens, everywhere. The account's, as Tips.
    {
      label: "Editor",
      submenu: EDITORS.map(([e, label]) => ({ label, hint: "What Open in editor opens", checked: editorShown() === e, run: () => setEditorPref(e) })),
    },
    ...(client?.state
      ? [
          "separator" as const,
          { label: "Getting started", run: () => openGettingStarted(undefined, client) } as MenuItem,
          // Features' help cards, opened on purpose (Tips on or off).
          { label: "Help", submenu: [{ id: "nvim-help", label: "nvim in Arugula", run: () => openHelpCard(client, undefined) }] } as MenuItem,
          { label: "What's new", hint: "The newest releases' headlines", run: () => void openWhatsNew(client) } as MenuItem,
          { label: "Command palette…", shortcut: keyHint("palette"), run: () => openPalette(client) } as MenuItem,
        ]
      : []),
  ];
}

/** What a hint's action (shell/hints) and What's new's *Try it*
 * (docs/releases' headline sections) may name, by id: each takes what the
 * hint's fact says; What's new gives none. */
export const COMMANDS: Record<string, (client: Client, args: Record<string, string | number>) => void> = {
  /** A port listening in a pane, opened beside it with nothing asked. */
  "port.open-beside": (client, { pane, port }) => {
    const mine = client.machine(Number(pane));
    openPortAt(client, { split: Number(pane), host: mine?.id, local: !mine }, Number(port));
  },
  "palette.open": (client) => openPalette(client),
  /** Getting started at its Agents step: Use Claude Code with Arugula. */
  "getting-started.agents": (client) => openGettingStarted("agents", client),
  /** Pin a board… beside the shown tab's pane. */
  "board.pin": (client) => {
    const pane = client.tab === null ? undefined : client.activePane.get(client.tab);
    if (pane !== undefined) void pinBoard(client, { from: pane });
    else client.toast("Open a pane first: a board pins beside one");
  },
  /** A PR's or an issue's link it printed, as a block beside it. */
  "forge.open-link": (client, { pane, url }) => void client.openForgeLink(Number(pane), String(url)),
  /** Pane settings › After a restart, under the pane's header. */
  "pane.restart-settings": (client, { pane }) => openMenu(under(`[data-pane-header="${pane}"]`), restartItems(client, Number(pane))),
  /** The last command's mark menu: Copy output, Run again. */
  "mark.menu": (client, { pane }) => void client.panes.get(Number(pane))?.view.openMarkMenu(),
  /** Pane › Share › Private, under the pane's header. */
  "pane.private": (client, { pane }) =>
    openMenu(under(`[data-pane-header="${pane}"]`), [
      { header: "Share" },
      ...driveItems(client, Number(pane)).filter((i) => typeof i === "object" && "label" in i && i.label.startsWith("Private")),
    ]),
  /** The Notifications items (what control pushes about), under the
   * account menu's button. */
  "push.kinds": (client) => openMenu(under("[data-account-menu]"), notificationItems(client)),
  /** Notify me about its agents (M29), for a session shared with you. */
  "agents.notify": (client, { session }) => void client.setNotify({ session: Number(session), on: true }),
  "changes.open": (client, { pane }) => openChanges(client, Number(pane)),
  "editor.open": (client, { pane }) => openEditor(client, Number(pane)),
  /** nvim's chips and help card (#1735, hints/nvim). */
  "nvim.install-plugin": (client, { pane }) => void addPlugin(client, Number(pane)),
  "editor.use-nvim": (client) => useNvim(client),
  "nvim.help-card": (client, { pane, section }) => openHelpCard(client, Number(pane), section === undefined ? undefined : String(section)),
  "nvim.help": (client, { pane, help }) => void openHelp(client, Number(pane), String(help)),
  "nvim.docs": () => void window.open(NVIM_DOCS, "_blank", "noopener"),
  /** A shell on another host, split right of the pane (#17). */
  "remote.split": (client, { pane, host }) => void newRemote(client, String(host), { split: Number(pane) }),
  /** Sign-in and account, where Add a passkey is. */
  "control.passkeys": () => openPanel("account")(),
  "pane.attach": (client, { pane }) => void client.attachFiles(Number(pane)),
  "pane.picker": (client, { pane }) => openPicker(client, Number(pane)),
  "push.enable": (client) => notifyThisDevice(client),
  /** Follow someone in the session (their client). */
  "people.follow": (client, { client: them }) => client.follow(Number(them)),
  "control.teams": () => openPanel("teams")(),
  "conversations.pick": (client) => {
    const pane = client.active();
    pickConversation(client, pane === undefined ? { session: client.session ?? undefined } : { split: pane, cwd: client.cwd(pane) ?? undefined });
  },
};

/** Where a menu opens under the element `selector` finds (else mid-page). */
function under(selector: string) {
  const at = document.querySelector(selector)?.getBoundingClientRect();
  return { clientX: at ? at.left + 8 : innerWidth / 2, clientY: at ? at.bottom : innerHeight / 3, preventDefault() {} };
}

/** The shown machine's settings: its standing permission rules (#166), its
 * usage and crash reports (M103) and its Labs flags (#665), the owner's.
 * In the machine menu. Developer settings are there whether or not the
 * machine has a `flags.json` yet: the dialog is how a flag gets turned on. */
export function machineSettingsItems(client: Client): MenuItem[] {
  if (!client.state || client.state.roles) return [];
  return [
    { label: "Permission rules…", run: () => openRules(client) },
    { label: "Usage and crash reports…", run: () => openMachineTelemetry(client) },
    // Native #205: a sharing setting, which was the `guest-ssh` flag.
    { label: "Guest SSH access…", run: () => openSharing(client) },
    // Discovery 10: what its agents may do on control (`arugula grants`),
    // once it's joined (or the page is control's).
    ...(directory.control || directory.joined ? [{ label: "Agents' grants…", run: openAgentGrants }] : []),
    { label: "Developer settings…", run: () => openDeveloper(client) },
  ];
}

/** "idle" when nothing in the tab runs on its machine any more. */
export function machineState(client: Client, m: Machine): string {
  return m.state === "running" && client.panesOn(m.id).length === 0 ? "idle" : m.state;
}

/** The tab's machine: how it is, a new pane on it, reset. */
function machineItems(client: Client, tab: TabView): MenuItem[] {
  const m = client.tabMachine(tab.id);
  if (!m) return [];
  const on = client.panesOn(m.id);
  const anchor = client.active(tab.id) ?? paneIds(tab)[0];
  return [
    "separator",
    { header: `Machine ${m.name ?? m.sprite} · ${machineState(client, m)}` },
    {
      label: "New pane on machine",
      disabled: anchor === undefined,
      run: () => anchor !== undefined && client.intent({ op: "split", pane: anchor, edge: "right", local: false }),
    },
    {
      label: "Open a port on machine…",
      disabled: anchor === undefined,
      run: () => anchor !== undefined && void openPort(client, { split: anchor, host: m.id }),
    },
    {
      label: "Reset machine",
      disabled: on.length === 0,
      run: () => void client.api(opPath("machine.reset", { id: m.id }), {}, "couldn't reset the machine"),
    },
  ];
}

/** What the pane does when the daemon starts again, e.g. after a reboot. */
function restartItems(client: Client, id: PaneId): MenuItem[] {
  const info = client.info(id);
  const p = info?.policy ?? { kind: "shell" };
  const cmd = info?.command;
  const set = (policy: Policy) => client.paneOp(id, { op: "set_policy", policy });
  const short = (s: string) => (s.length > 32 ? `${s.slice(0, 31)}…` : s);
  return [
    { header: "After a restart" },
    { label: "Start a shell here", checked: p.kind === "shell", run: () => set({ kind: "shell" }) },
    {
      label: cmd ? `Re-run ${short(cmd)}, asking first` : "Re-run the command, asking first",
      checked: p.kind === "rerun" && p.confirm,
      run: () => set({ kind: "rerun", confirm: true }),
    },
    {
      label: cmd ? `Re-run ${short(cmd)}` : "Re-run the command",
      checked: p.kind === "rerun" && !p.confirm,
      run: () => set({ kind: "rerun", confirm: false }),
    },
    {
      label: p.kind === "hook" ? `Run ${short(p.command)}` : "Run a command…",
      checked: p.kind === "hook",
      run: async () => {
        const command = await askText("Run when restored", p.kind === "hook" ? p.command : "", "claude --continue");
        if (command?.trim()) set({ kind: "hook", command: command.trim() });
      },
    },
    {
      label: info?.resumes ? `Resume ${short(info.resumes)}` : "Resume the agent's conversation",
      checked: p.kind === "resume",
      run: () => set({ kind: "resume" }),
    },
    { label: "Nothing (wait for Enter)", checked: p.kind === "none", run: () => set({ kind: "none" }) },
  ];
}
