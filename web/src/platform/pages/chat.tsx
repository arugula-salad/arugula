// `/p/:project/chat/…` and `/dm/:room`: chat (M150), the same rooms view
// the shell draws (../chat/rooms), here with paths for its links. Pane and
// session threads show too; their chips open on control's page.

import { maintenance, MAINTENANCE_SAID } from "../../shared/maintenance";
import { useSubscribe } from "../../shared/ui/hooks";
import type { ChatRoute } from "../../shared/chat/route";
import { RoomsView, useChat } from "../chat/rooms";
import { PlatformBar } from "../bar";

export function ChatPage({ route }: { route: ChatRoute }) {
  const store = useChat();
  useSubscribe((fn) => maintenance.subscribe(fn));
  if (store) return <RoomsView route={route} store={store} />;
  return (
    <div class="chat" data-chat data-chat-off>
      <PlatformBar />
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
