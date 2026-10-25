// What the platform's views (chat: web/src/platform/) reach the page
// they're drawn on through (M147). The shell gives one built on its fleet
// and its control session (web/src/shell/platform-api.tsx); the platform
// page (M150) gives one with no machines at all.
// The views never import the fleet, the daemon client or control's signing
// code: what they need of them is here, and CI checks it (M148).

import { createContext, type ComponentChildren, type ComponentType } from "preact";
import { useContext } from "preact/hooks";
import type { PaneQuote } from "./chat-store";

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
  openChatView(view: "activity" | "saved" | "search" | "threads", q?: string): void;
  /** A message's link, to copy; a reply's names its thread. */
  roomLink(room: string, seq: number, thread?: number): string;
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
  /** A session's name, when this page sees it; "" otherwise. */
  sessionName(machine: string | null | undefined, session: number | string): string;
  /** Show a pane (with the quote found in it), or a session. Says why not
   * when it can't. */
  goToPane(machine: string, pane: string, quote: string | null): void;
  goToSession(machine: string): void;
  /** The page's bar, with what a view puts in it. */
  Bar: ComponentType<{ children?: ComponentChildren }>;
  /** Pick lines from one of the reader's panes to quote, where the page
   * reads panes; `can` says whether a machine is there to quote from. */
  quote?: { can(): boolean; Picker: ComponentType<{ close: () => void; take: (q: PaneQuote) => void }> };
  /** Activity's "Needs you": what wants the reader on every machine, where
   * the page sees machines. `count` says whether it shows anything. */
  needsYou?: { count(): number; View: ComponentType };
}

const none = () => () => {};

/** A page with no machines: what the platform page has, and the default. */
export const noMachines: Pick<PlatformApi, "machine" | "paneTitle" | "sessionName" | "goToPane" | "goToSession"> = {
  machine: () => undefined,
  paneTitle: () => "",
  sessionName: () => "",
  goToPane: () => {},
  goToSession: () => {},
};

export const PlatformApiContext = createContext<PlatformApi>({
  teams: () => [],
  subscribe: none,
  toast: () => {},
  nav: { openChat() {}, closeChat() {}, openRoom() {}, openChatView() {}, roomLink: () => "" },
  ...noMachines,
  Bar: ({ children }) => <>{children}</>,
});

export const usePlatformApi = () => useContext(PlatformApiContext);
