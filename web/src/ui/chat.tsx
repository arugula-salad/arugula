// The chat page (`/#chat`, M73): a page of its own beside the panes and the
// swarm. Since M123 it is control's chat (./chat-rooms: a team's channels,
// DMs, and each pane's and session's thread as a room), and since M125 the
// only chat: no machine keeps threads of its own.

import type { Client } from "../client";
import { chatRoute, useChatTick } from "./places";
import { RoomsView, useChat } from "./chat-rooms";

export function ChatPage({ client }: { client: Client }) {
  useChatTick(client);
  const route = chatRoute();
  const store = useChat();
  if (!store || !route) return null;
  return <RoomsView client={client} route={route} store={store} />;
}
