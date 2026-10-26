// `/p/:project/chat/…` and `/p/:project/dm/:room`: chat (M150), the same
// rooms view the shell can draw (../../shared/chat/app), here with paths
// for its links (../api). Pane and session threads show too; their chips
// open on control's page.
//
// UI audit #169: a project's chat is its own rooms, a team's channels and
// DMs, or (Personal) the reader's machines' threads; a room of another
// project's opens in that one (../page's useSlug). It sits in the page's
// frame, as every other page does: the bar, then the rooms in a panel
// inset from the window's edges (platform.css, `.platform-chat`). A
// phone's chat is the whole screen, as it is in the shell.

import type { ComponentChildren } from "preact";
import type { ChatRoute } from "../../shared/chat/route";
import { ChatApp } from "../../shared/chat/app";
import type { ChatScope } from "../../shared/chat/sidebar";
import { useSubscribe } from "../../shared/ui/hooks";
import { session } from "../session";

export function ChatPage({ project, route, children }: { project: string; route: ChatRoute; children?: ComponentChildren }) {
  useSubscribe((fn) => session.subscribe(fn));
  const p = session.project(project);
  const scope: ChatScope | undefined = p ? { team: p.team?.id ?? null, name: p.name } : undefined;
  return (
    <div class="platform platform-chat" data-platform-chat>
      <ChatApp route={route} scope={scope} />
      {children}
    </div>
  );
}
