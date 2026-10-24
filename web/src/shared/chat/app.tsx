// Team chat as one piece (#123): the sidebar and the room, as the platform
// page shows them, for any host to mount: the platform page today, a
// multiplexer block later. Not mounted by the shell yet.
//
// What a host provides:
//  - a `PlatformApiContext` (../platform-api): `nav` (where a room, a
//    message, Activity and search go, and the link to copy), `toast`,
//    `teams()` and `subscribe`, the machines it can see (or `noMachines`),
//    and `Bar`, the strip search sits in;
//  - control's chat signed in as the reader: `chat.on` (../chat-store) is
//    true when it is on for them, and `maintenance` says when control is down;
//  - the `route` it is showing, read from its own address: a host with no
//    address of its own (a block) keeps it in its layout entry;
//  - `.chat` styles and a box with a height to fill.

import { maintenance, MAINTENANCE_SAID } from "../maintenance";
import { useSubscribe } from "../ui/hooks";
import { usePlatformApi } from "../platform-api";
import type { ChatRoute } from "./route";
import { RoomsView, useChat } from "./rooms";

export function ChatApp({ route }: { route: ChatRoute }) {
  const { Bar } = usePlatformApi();
  const store = useChat();
  useSubscribe((fn) => maintenance.subscribe(fn));
  if (store) return <RoomsView route={route} store={store} />;
  return (
    <div class="chat" data-chat data-chat-off>
      <Bar />
      <div class="chat-body">
        <div class="chat-thread chat-empty">
          {maintenance.down ? (
            <p data-maintenance-note>{MAINTENANCE_SAID}</p>
          ) : (
            <p>Chat is off for you. A team owner turns it on in the team's Labs, on control's page, for the team's channels and messages.</p>
          )}
        </div>
      </div>
    </div>
  );
}
