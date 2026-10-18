// The bar over the panes: the shared app bar (machine, places, commands,
// account) with the session, Share and the tab strip in it. M108 (#1023)
// turns it into the context header.

import type { Client } from "../../client";
import { AppBar } from "../app-bar";
import { CloudTabs } from "../cloud";
import { PeopleBar, shareSession } from "../people";
import type { Renaming } from "./rename-input";
import { SessionButton } from "./session-sidebar";
import { TabStrip } from "./tab-strip";

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
