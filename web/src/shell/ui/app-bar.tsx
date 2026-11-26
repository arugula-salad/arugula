// The bar every place shares: the panes, the swarm, chat and agents. The
// same things sit in the same spots on each: the machine and the places on
// the left, what the place itself offers next to them, then the command
// palette's button and the account menu on the right. Before this each
// place drew a bar of its own, and the swarm none.

import type { ComponentChildren } from "preact";
import type { Client } from "../client";
import { usePhone, useSubscribe } from "../../shared/ui/hooks";
import { HostButton } from "./hosts";
import { MachinePlaces, Places, type Place } from "./places";
import { AttentionBell } from "./shell/attention-drawer";
import { ConnectionChip } from "./shell/context-header";
import { openMenu, useMenuOpenFrom, type MenuItem } from "../../shared/ui/menu";
import { useRef } from "preact/hooks";
import { settingsItems } from "./commands";
import { openSettings } from "./settings-route";
import { ariaKeys, keyHint } from "../../shared/keys";
import { openGoTo } from "../../shared/kit/frame/goto";
import { openFeedback } from "./feedback";
import { UpdateChip } from "./update";
import { WindowButtons } from "../../shared/ui/window-buttons";
import { countView } from "../../shared/telemetry";
import { AccountPicture } from "../../shared/chat/avatar";
import { Avatar } from "../../shared/kit/avatar";
import { Caret, SearchIcon } from "../../shared/ui/icons";

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

/** The account now, for Go to…'s Account commands (UI audit
 * #176); null on a daemon's own page. */
export const currentAccount = (): Account | null => account;

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
  /** The place shows every machine (the swarm):
   * the machine picker would read as a filter it isn't, so it says so. */
  all?: boolean;
}) {
  useSubscribe((fn) => client.subscribe(fn));
  // The workspace's rail has the whole app's places (M108), on a computer
  // once a machine is shown (app.tsx), and the bar this machine's.
  const rail = !usePhone() && !!client.state;
  return (
    <header class="bar app-bar" data-tauri-drag-region inert={inert} data-at={at}>
      {all ? (
        <span class="scope-label" data-scope="all" title="This shows every machine. Panes shows one: pick it there.">
          All machines
        </span>
      ) : rail ? (
        <>
          <HostButton path={false} />
          <ConnectionChip client={client} />
        </>
      ) : (
        <HostButton />
      )}
      {!rail && <Places client={client} at={at} />}
      {rail && <MachinePlaces client={client} at={at} />}
      {children}
      <div class="bar-fill" data-tauri-drag-region />
      {client.state && <CommandsButton />}
      <UpdateChip client={client} />
      {/* What waits on you, on every machine, in every bar (Arugula #637). */}
      {rail && <AttentionBell client={client} />}
      {end}
      {/* With the rail, you're its foot (Lift 3). */}
      {!rail && <AccountButton client={client} />}
      <WindowButtons />
    </header>
  );
}

/** Go to…, where it can be seen: a place, a pane, or a command a pane can do
 * (the rest are in its right-click menu). */
export function CommandsButton({ search }: { search?: boolean }) {
  return (
    <button
      class={search ? "commands-button ws-search-button" : "commands-button"}
      data-open-palette
      // One label and title in every bar (UI audit #170); the panes' head
      // draws it as a search field.
      title={`Go to a place, a pane or a command (${keyHint("goto")})`}
      // No aria-label: its name is what it shows, "Go to…" and the chord,
      // so voice control's "click Go to" and a screen reader agree.
      aria-keyshortcuts={ariaKeys(keyHint("goto"))}
      onClick={() => openGoTo()}
    >
      {search && <SearchIcon />}
      <span>Go to…</span>
      {/* A space between them, so the name reads "Go to… Ctrl+K". */}
      {" "}
      <kbd>{keyHint("goto")}</kbd>
    </button>
  );
}

/** The account (through control) and the settings that aren't any one
 * session's: this machine's, this device's, and help. On the rail's foot
 * (`onRail`), your initials (the kit's Avatar). */
export function AccountButton({ client, onRail = false }: { client: Client | null; onRail?: boolean }) {
  const account = useAccount();
  const login = account?.login() ?? null;
  const ref = useRef<HTMLButtonElement>(null);
  const expanded = useMenuOpenFrom(ref);
  // Where the rail has Settings, this is who you are (Arugula #695); with no
  // rail (a phone, no machine shown) it's the way to the settings too.
  const rail = !usePhone() && !!client?.state;
  const open = (e: MouseEvent) => {
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    const mine: MenuItem[] = account ? [{ header: accountHeader(login), name: !!login }, ...account.items(), "separator"] : [];
    const settings: MenuItem[] = rail ? [{ label: "Settings", run: () => openSettings() }] : settingsItems(client);
    if (!rail) countView("settings");
    // Your presence is Chat's "you" row (Arugula #1079), not this menu's.
    const items: MenuItem[] = [...mine, ...settings];
    // Arugula #1217; Go to… has it too.
    if (client?.state) items.push("separator", { label: "Send feedback…", run: () => openFeedback(client) });
    if (account) items.push("separator", { label: "Sign out", run: () => account!.signOut() });
    // Right-aligned under the button (beside it on the rail); the menu keeps itself on screen.
    openMenu({ clientX: onRail ? r.right + 4 : r.right, clientY: onRail ? r.top : r.bottom + 4, preventDefault: () => e.preventDefault() }, items, e.currentTarget as HTMLElement);
  };
  // A daemon's own page has no account: the rail's Settings is all there is.
  if (rail && !account) return null;
  if (onRail)
    return (
      <button ref={ref} type="button" class="k-rail-me" aria-haspopup="menu" aria-expanded={expanded} title="Your account" data-account-menu data-signed-in-as onClick={open} onContextMenu={open}>
        <Avatar name={login ?? "?"} />
      </button>
    );
  return (
    <>
      <button
        ref={ref}
        class="account-button"
        aria-haspopup="menu"
        aria-expanded={expanded}
        title={login ? "Your account" : "Settings"}
        aria-label={login ? undefined : "Settings"}
        data-account-menu
        onClick={open}
        onContextMenu={open}
      >
        <span class="account-avatar" aria-hidden="true">
          {login ? login.slice(0, 1).toUpperCase() : "⚙"}
          {login && <AccountPicture />}
        </span>
        {login && <span class="account-login" data-signed-in-as>{login}</span>} <Caret />
      </button>
    </>
  );
}
