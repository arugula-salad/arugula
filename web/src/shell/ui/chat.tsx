// The chat page (`/#chat`, M73): a page of its own beside the panes and the
// swarm. Since M123 it is control's chat (./chat-rooms: a team's channels,
// DMs, and each pane's and session's thread as a room), and since M125 the
// only chat: no machine keeps threads of its own. Without it (a machine
// with the `chat` Labs flag, reached on its own or with chat off in
// control), the page says chat is control's (S41 decision 7).

import type { Client } from "../client";
import { chat } from "../../shared/chat-store";
import { maintenance, MAINTENANCE_SAID } from "../../shared/maintenance";
import { AppBar } from "./app-bar";
import { useSubscribe } from "../../shared/ui/hooks";
import { chatRoute, useChatTick } from "./places";
import { RoomsView, useChat } from "../../platform/chat/rooms";

export function ChatPage({ client }: { client: Client }) {
  useChatTick(client);
  const route = chatRoute();
  const store = useChat();
  if (!route) return null;
  if (!store) return <ChatNeedsControl client={client} />;
  return <RoomsView client={client} route={route} store={store} />;
}

/** Chat with no chat in control: where it went, and how to get it. */
function ChatNeedsControl({ client }: { client: Client }) {
  useSubscribe((fn) => maintenance.subscribe(fn));
  return (
    <div class="chat" data-chat data-chat-needs-control>
      <AppBar client={client} at="chat" all />
      <div class="chat-body">
        <div class="chat-thread chat-empty">
          {maintenance.down ? (
            // A maintenance window (M141) before chat first loaded: not off.
            <p data-maintenance-note>{MAINTENANCE_SAID}</p>
          ) : chat.signedIn ? (
            <p>
              Chat is off for you in control. Turn on Chat in control's Labs (yours, or a team owner's for the team) to
              talk in your teams' channels, in messages, and on each pane and session. This machine's old threads come
              with it.
            </p>
          ) : (
            <p>
              Chat needs control: it's kept there, for your teams, and no machine keeps threads of its own. Join this
              machine to control and turn on Chat in its Labs, and its old threads come with it.
            </p>
          )}
        </div>
      </div>
    </div>
  );
}
