// What the platform's views (chat: web/src/platform/) reach the page
// they're drawn on through (M147). The shell gives one built on its fleet
// and its control session (web/src/shell/platform-api.tsx); the platform
// page (M150) gives one with no machines at all.
// The views never import the fleet, the daemon client or control's signing
// code: what they need of them is here, and CI checks it (M148).

import { createContext, type ComponentChildren, type ComponentType } from "preact";
import { useContext } from "preact/hooks";
import type { PaneQuote } from "./chat-store";
import type { MachineAt } from "./machine-paths";
import type { AgentPanelProps } from "./kit/panel";
import type { Inbox } from "./proto";

/** Where a project's Overview is zoomed in to (Arugula #1287): a board
 * (its Board level), a card on it, or a session on no card by its key on
 * the Overview (its Work level). None: its Project level. */
export type OverviewZoom = { board: string; card?: string } | { session: string };

/** A team this account is in, as the platform's views read it: control's
 * `Team` has these and more. */
export interface PlatformTeam {
  team: string;
  role: string | null;
  roster: { name: string; members: { account: string; name: string }[] };
  /** What the team calls each member, by account. */
  names?: Record<string, string>;
}

/** One of the reader's machines, by control's id for it. */
export interface PlatformMachine {
  name: string;
  /** Whether it answers now. */
  up: boolean;
}

/** Where the platform's chat links and buttons go: the shell's `#chat`
 * routes, or the platform page's paths. */
export interface ChatNav {
  /** Every room, from the top. */
  openChat(): void;
  closeChat(): void;
  /** A room, at message `msg`; with `thread`, the thread of that message
   * beside it (#106), and `msg` a reply in it to show. */
  openRoom(room: string, msg?: number, thread?: number): void;
  /** Activity, the threads I follow, or search results for `q`. */
  openChatView(view: "activity" | "saved" | "search" | "threads", q?: string, all?: boolean): void;
  /** A phone's swipe between levels (#816); a host that omits them has none.
   * `up`: where a swipe right goes (null: nowhere). `down`: where a swipe
   * left goes (null: nowhere, the drag rubber-bands). `goTo` commits one. */
  up?(): string | null;
  down?(): string | null;
  goTo?(path: string): void;
  /** Open chat where the person left it (#965): the room (and thread), with
   * the levels above it beneath, so Back climbs. A host that omits it
   * opens the list. */
  resume?(room: string, thread?: number): void;
  /** Whether chat was opened from inside the app (the rail's Chat, the Chat
   * tab) rather than by its address; says so once per opening. On a phone
   * only these resume: a page load of the list is the list. */
  openedInApp?(): boolean;
  /** A message's link, to copy; a reply's names its thread. */
  roomLink(room: string, seq: number, thread?: number): string;
  /** A room's address, for its row's `href` (UI audit #185). */
  roomHref(room: string): string;
  /** Where the reader gives their machines' agents access to rooms (a
   * room's agents chip links here); a host with no such page omits it. */
  openGrants?(): void;
}

export interface PlatformApi {
  /** The teams this account is in; none until they're known. */
  teams(): PlatformTeam[];
  /** Re-render on what the getters here read (teams, machines, panes). */
  subscribe(fn: () => void): () => void;
  toast(text: string): void;
  nav: ChatNav;
  /** The reader's machine, when this page reaches it. */
  machine(id: string | null | undefined): PlatformMachine | undefined;
  /** What a pane does, when this page sees it; "" otherwise. */
  paneTitle(machine: string | null | undefined, pane: number | string): string;
  /** A project's name, when this page has its projects (Arugula #1457):
   * a chief of staff's session is named by its project's id, and shown by
   * this. None: such a name shows as it is. */
  projectName?(id: string): string | undefined;
  /** A session's name, when this page sees it; "" otherwise. */
  sessionName(machine: string | null | undefined, session: number | string): string;
  /** Show a pane (with the quote found in it), or a session. Says why not
   * when it can't. */
  goToPane(machine: string, pane: string, quote: string | null): void;
  goToSession(machine: string): void;
  /** The pane an agent's tab (`session`, as its post named it) is, on a
   * machine this page reaches; undefined when the page can't open it (not
   * the reader's, not shared with them, or no such tab now). `up` says
   * whether the machine answers. */
  agentPane(machine: string, session: string): { pane: string; up: boolean; mentionsOff?: boolean } | undefined;
  /** Act on the reader's own agent's pane (#653), through this page's own
   * connection to its machine: silence mentions of it,
   * allow them again, or stop its turn. Says why not when it can't. A page
   * with no machines leaves it out. */
  agentAct?(machine: string, pane: string, act: "silence" | "allow" | "stop"): Promise<string | null>;
  /** The page's bar, with what a view puts in it. */
  Bar: ComponentType<{ children?: ComponentChildren }>;
  /** Pick lines from one of the reader's panes to quote, where the page
   * reads panes; `can` says whether a machine is there to quote from. */
  quote?: { can(): boolean; Picker: ComponentType<{ close: () => void; take: (q: PaneQuote) => void }> };
  /** Activity's "Needs you": what wants the reader on every machine, where
   * the page sees machines. `count` says whether it shows anything. */
  needsYou?: { count(): number; View: ComponentType };
  /** Where control's page opens `hash` (`pane=<machine>.<pane>`,
   * `machine=<id>`), for a page that isn't control's own; the shell omits
   * it and opens panes itself (`goToPane`). */
  controlHref?(hash: string): string;
  /** An agent session's own page (M257), where the page has one. */
  sessionHref?(project: string, ses: string): string;
  /** Docs kept on a machine (#517): the shell opens one in a block when it is on that machine (`here`), beside the board block's pane when `beside` is given, and the platform page omits this. */
  machineDoc?: { here(machine: string): boolean; open(machine: string, doc: string, beside?: number): void };
  /** A machine's library Docs, which only that machine moves (its `doc
   * move`, Arugula #971), where the page reaches the reader's own machines
   * (the app's): `watch` reads them, calling `fn` as they change, until its
   * answer is called; `here` says whether the page reaches `machine` now;
   * `move` gives one Doc to a team's project, and says why not when it
   * can't. A page without it sends the person to the machine. */
  machineDocs?: { watch(fn: () => void): () => void; here(machine: string): boolean; move(machine: string, doc: string, project: string): Promise<string | null> };
  /** The Docs the reader shared by link (Arugula #1315), as the machine
   * that shared each (control's id) knows them: its title, its URL (which
   * holds the key) and its pane. Where the page reaches the reader's own
   * machines (the app). `watch` reads them, calling `fn` as they change,
   * until its answer is called; `here` says whether `machine` answers now,
   * and `name` what it's called; `titles` is each link's Doc title, by
   * link id; `url` and `open` (the pane its Doc opened in) throw why not;
   * `unshare` is whether the machine took the link down itself. A page
   * without it shows the links with neither Open nor Copy link. */
  docLinks?: {
    watch(fn: () => void): () => void;
    here(machine: string): boolean;
    name(machine: string): string | undefined;
    titles(machine: string): Promise<Map<string, string>>;
    url(machine: string, link: string): Promise<string>;
    open(machine: string, link: string): Promise<number>;
    unshare(machine: string, link: string): Promise<boolean>;
  };
  /** One project's Inbox (Arugula #1469), where the page reads it (the
   * platform's): a board's Needs you and its card take the agents' items on
   * their issues from it (`subject.issue`). `me` is the reader's account,
   * `name` a teammate's name, `answer` an approval's or a to-do's choice
   * (`POST /api/inbox/{id}`), throwing the server's words. A page without
   * it draws board asks alone. */
  inbox?: {
    read(project: string): Promise<Inbox>;
    me(): string | null;
    name(account: string): string;
    answer(id: string, choice: string, note?: string): Promise<void>;
  };
  /** Whether pane `pane` of machine `machine` is in the fleet this page
   * reaches and the reader's own, as the Overview's peek tells it (a
   * board's lead, Arugula iss_89b78ece43ac3d6271f78d797cdbdc61): `watch`
   * reads the machines, calling `fn` as they change, until its answer is
   * called. Where the page reaches machines itself (the app); a page
   * without it opens no lead's pane. */
  ownPane?: { watch(fn: () => void): () => void; here(machine: string, pane: number): boolean };
  /** A live pane in a fixed slot: the shell draws its own PaneSlot; a page that can't draw one leaves it out and the drawer shows the session's state card. */
  LivePane?: ComponentType<{ machine: string; pane: string }>;
  /** A pane at the Overview's Work level (Arugula #1289): the view Panes mounts, filling its slot, typed into unless `watch` (a teammate's session) or the reader only views it. Where the page reaches machines itself (the app). */
  LiveWork?: ComponentType<{ machine: string; pane: string; watch?: boolean }>;
  /** A pane's question and its choices, answered in place as Swarm's cards answer them; nothing while it asks none. Where the page reaches machines itself (the app): a board drawer draws it at its top, and with it says when a row has no session yet. Control's page leaves it out. */
  LiveAsk?: ComponentType<{ machine: string; pane: string }>;
  /** The bottom of a board drawer whose live session is an agent block on a machine (Arugula #1019): why its pane's session is on this card, where the page can tell, and Change card, the pane chip's picker on that session. Where the page reaches machines itself (the app); a page without it shows neither. */
  CardWhy?: ComponentType<{ machine: string; pane: string }>;
  /** The machines, a machine's page and Swarm, in a project (Arugula
   * #989): the app's, where it reaches machines. A page without it sends
   * them to control's page. */
  Machines?: ComponentType<{ project: string; at: MachineAt }>;
  /** A project's Overview (Arugula inversion, L6): its open boards, a row
   * each, with their cards in flight and the sessions on them, and Needs
   * you, where the page reaches machines; `board` and `card` name the card
   * whose drawer is open. Its children are the project's page as it was,
   * shown only where there is no Overview (the page doesn't reach
   * machines, #1088). A page without it shows those alone. */
  Overview?: ComponentType<{ project: string; board?: string; card?: string; zoom?: OverviewZoom; children?: ComponentChildren }>;
  /** The Inbox (Lift 6d, #1486): every project's items, answered in place,
   * a pane's ask through its machine where the page reaches machines. */
  Inbox?: ComponentType;
  /** The agent panel (Lift 4, #1480): the project's chief of staff first,
   * then agents, rooms and reply threads as tabs (kit/panel.tsx's
   * `usePanel`). Where the page reaches machines itself (the app); a page
   * without it has no panel. */
  AgentPanel?: ComponentType<AgentPanelProps>;
  /** Settings › Machines' sections (Arugula #1107): Access and sharing,
   * Agents and Advanced, whose parts are mostly the machine's own, read
   * from it through the page's own client to it, each saying so when the
   * page can't reach it. `grants`, the platform's part of Agents;
   * `children`, its sections about the machine (Name), go before
   * Advanced. Where the page reaches machines itself (the app); a page
   * without it draws the platform's alone, and says the rest aren't there. */
  MachineSettings?: ComponentType<{ machine: string; name: string; grants?: ComponentChildren; children?: ComponentChildren }>;
}

const none = () => () => {};

/** A page with no machines: what the platform page has, and the default. */
export const noMachines: Pick<PlatformApi, "machine" | "paneTitle" | "sessionName" | "goToPane" | "goToSession" | "agentPane"> = {
  machine: () => undefined,
  paneTitle: () => "",
  sessionName: () => "",
  goToPane: () => {},
  goToSession: () => {},
  agentPane: () => undefined,
};

export const PlatformApiContext = createContext<PlatformApi>({
  teams: () => [],
  subscribe: none,
  toast: () => {},
  nav: { openChat() {}, closeChat() {}, openRoom() {}, openChatView() {}, roomLink: () => "", roomHref: () => "" },
  ...noMachines,
  Bar: ({ children }) => <>{children}</>,
});

export const usePlatformApi = () => useContext(PlatformApiContext);
