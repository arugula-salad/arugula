// `/p/:project/chat/…` and `/dm/:room`: chat (M150), the same rooms view
// the shell can draw (../../shared/chat/app), here with paths for its links
// (../api). Pane and session threads show too; their chips open on
// control's page.

import type { ChatRoute } from "../../shared/chat/route";
import { ChatApp } from "../../shared/chat/app";

export function ChatPage({ route }: { route: ChatRoute }) {
  return <ChatApp route={route} />;
}
