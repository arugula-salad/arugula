// Team chat as one piece (#123): the sidebar and the room, as the platform
// page shows them, for any host to mount: the platform page, and the
// shell's chat block (../../shell/blocks/team-chat).
//
// What a host provides:
//  - a `PlatformApiContext` (../platform-api): `nav` (where a room, a
//    message, Activity and search go, and the link to copy), `toast`,
//    `teams()` and `subscribe`, the machines it can see (or `noMachines`),
//    and `Bar`, the strip search sits in;
//  - control's chat signed in as the reader: `chat.on` (../chat-store) is
//    true when it is on for them (null while not known yet, `signedOut`
//    when control no longer knows the session), and `maintenance` says
//    when control is down;
//  - the `route` it is showing, read from its own address: a host with no
//    address of its own (a block) keeps it in its layout entry;
//  - `.chat` styles and a box with a height to fill; `fit` says how it
//    fits that box (a narrow one, a single pinned room, one of several).

import { maintenance, MAINTENANCE_SAID } from "../maintenance";
import { useSubscribe } from "../ui/hooks";
import { chat } from "../chat-store";
import { usePlatformApi } from "../platform-api";
import type { ChatRoute } from "./route";
import { RoomsView, useChat, type ChatFit } from "./rooms";
import type { ChatScope } from "./sidebar";

/** `scope`: a page of one project's rooms (the platform page's, UI audit
 * #169); none for every room. */
export function ChatApp({ route, fit, scope }: { route: ChatRoute; fit?: ChatFit; scope?: ChatScope }) {
  const { Bar } = usePlatformApi();
  const store = useChat();
  useSubscribe((fn) => maintenance.subscribe(fn));
  if (store) return <RoomsView route={route} store={store} fit={fit} scope={scope} />;
  // Not known yet is not off (UI audit #143): "off" only once control said
  // so, and a session control no longer knows says that instead.
  let note;
  if (maintenance.down) note = <p data-maintenance-note>{MAINTENANCE_SAID}</p>;
  else if (chat.on === null) {
    note = chat.unreachable ? (
      <p data-chat-unreachable>Can't reach chat on Arugula control. Trying again…</p>
    ) : (
      <p class="dim" data-chat-loading>
        Loading chat…
      </p>
    );
  } else if (chat.signedOut) {
    note = (
      <p data-chat-signed-out>
        You're signed out of Arugula control.{" "}
        <button class="primary" onClick={() => location.reload()}>
          Sign in again
        </button>
      </p>
    );
  } else note = <p>Chat is off for you. A team owner turns it on for the team under Teams, on Control: its channels, messages and threads.</p>;
  return (
    <div class="chat" data-chat data-chat-off={chat.on === false && !chat.signedOut ? "" : undefined}>
      <Bar />
      <div class="chat-body">
        <div class="chat-thread chat-empty">{note}</div>
      </div>
    </div>
  );
}
