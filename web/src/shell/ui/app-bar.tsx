// The bar every place shares: the panes, the swarm, chat and agents. The
// same things sit in the same spots on each: the machine and the places on
// the left, what the place itself offers next to them, then the command
// palette's button and the account menu on the right. Before this each
// place drew a bar of its own, and the swarm none.

import type { ComponentChildren } from "preact";
import type { Client } from "../client";
import { usePhone, useSubscribe } from "../../shared/ui/hooks";
import { HostButton } from "./hosts";
import { Places, type Place } from "./places";
import { openMenu, type MenuItem } from "../../shared/ui/menu";
import { settingsItems } from "./commands";
import { ariaKeys, keyHint } from "./keys";
import { openPalette } from "./palette";
import { UpdateChip } from "./update";
import { WindowButtons } from "../../shared/ui/window-buttons";
import { countView } from "../../shared/telemetry";
import { AccountPicture } from "../../shared/chat/avatar";
import { presenceItems } from "../../shared/chat/presence";
import { SearchIcon } from "./shell/icons";
import { openPlatform, platformHref } from "../platform-links";

/** Who is signed in, through control: their name and their account's
 * items. A daemon's own page has none, and its menu is settings only. */
export interface Account {
  login: () => string | null;
  items: () => MenuItem[];
  signOut: () => void;
  subscribe: (fn: () => void) => () => void;
}

let account: Account | null = null;
export function setAccount(a: Account) {
  account = a;
}

/** The account, kept current; null on a daemon's own page. The bar's menu
 * and the phone's sheet (#820) both show its items. */
export function useAccount(): Account | null {
  useSubscribe((fn) => account?.subscribe(fn) ?? (() => {}));
  return account;
}

/** What the account menu and the phone's sheet head the account's items
 * with. */
export const accountHeader = (login: string | null) => (login ? `Signed in as ${login}` : "Your account");

export function AppBar({
  client,
  at,
  inert,
  children,
  end,
  all,
}: {
  client: Client;
  at: Place;
  inert?: boolean;
  /** What this place offers: the panes' sessions and tabs, chat's search. */
  children?: ComponentChildren;
  /** Before the account menu: the panes' people. */
  end?: ComponentChildren;
  /** The place shows every machine (the swarm, chat, the team's agents):
   * the machine picker would read as a filter it isn't, so it says so. */
  all?: boolean;
}) {
  useSubscribe((fn) => client.subscribe(fn));
  // The workspace's rail has the places (M108), on a computer once a
  // machine is shown (app.tsx).
  const rail = !usePhone() && !!client.state;
  return (
    <header class="bar app-bar" data-tauri-drag-region inert={inert} data-at={at}>
      {all ? (
        <span class="scope-label" data-scope="all" title="This shows every machine. Panes shows one: pick it there.">
          All machines
        </span>
      ) : (
        <HostButton />
      )}
      {!rail && <Places client={client} at={at} />}
      {children}
      <div class="bar-fill" data-tauri-drag-region />
      {client.state && <CommandsButton client={client} />}
      <UpdateChip client={client} />
      {end}
      <ProjectsLink />
      <AccountButton client={client} />
      <WindowButtons />
    </header>
  );
}

/** The command palette, where it can be seen: most of what a pane can do
 * is in it (and in a pane's right-click menu). */
export function CommandsButton({ client, search }: { client: Client; search?: boolean }) {
  return (
    <button
      class={search ? "commands-button ws-search-button" : "commands-button"}
      data-open-palette
      // One label and title in every bar (UI audit #170); the panes' head
      // draws it as a search field.
      title={`Every command for this pane, tab and session (${keyHint("palette")})`}
      // Its name is "Commands": the chord beside it is for the eye, and
      // would otherwise make it a button named "…+Shift+P".
      aria-label="Commands"
      aria-keyshortcuts={ariaKeys(keyHint("palette"))}
      onClick={() => openPalette(client)}
    >
      {search && <SearchIcon />}
      <span>Commands</span>
      <kbd aria-hidden="true">{keyHint("palette")}</kbd>
    </button>
  );
}

/** The way to the platform page, as its bar's "Machines ↗" comes here
 * (UI audit #170): only where control has one. */
export function ProjectsLink() {
  const href = platformHref("/");
  if (!href) return null;
  return (
    <a
      class="place ws-cross"
      href={href}
      target="arugula-platform"
      title="Your projects, their issues, runs and chat, on the platform page"
      data-open-projects
      onClick={(e) => {
        // Its own tab, or this window in the app (openPlatform); a modified
        // click is the browser's.
        if (e.button !== 0 || e.metaKey || e.ctrlKey || e.shiftKey || e.altKey) return;
        e.preventDefault();
        openPlatform("/");
      }}
    >
      Projects ↗
    </a>
  );
}

/** The account (through control) and the settings that aren't any one
 * session's: this machine's, this device's, and help. */
export function AccountButton({ client }: { client: Client | null }) {
  const account = useAccount();
  const login = account?.login() ?? null;
  const open = (e: MouseEvent) => {
    countView("settings");
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    const mine: MenuItem[] = account ? [{ header: accountHeader(login) }, ...account.items(), "separator"] : [];
    const items: MenuItem[] = [...mine, ...presenceItems(), ...settingsItems(client)];
    if (account) items.push("separator", { label: "Sign out", run: () => account!.signOut() });
    // Right-aligned under the button; the menu keeps itself on screen.
    openMenu({ clientX: r.right, clientY: r.bottom + 4, preventDefault: () => e.preventDefault() }, items);
  };
  return (
    <button
      class="account-button"
      title={login ? "Your account and settings" : "Settings"}
      aria-label={login ? `Account: ${login}` : "Settings"}
      data-account-menu
      onClick={open}
      onContextMenu={open}
    >
      <span class="account-avatar" aria-hidden="true">
        {login ? login.slice(0, 1).toUpperCase() : "⚙"}
        {login && <AccountPicture />}
      </span>
      {login && <span class="account-login" data-signed-in-as>{login}</span>} <span class="caret">▾</span>
    </button>
  );
}
