// The sessions on the shown machine. M108 (#1023) makes this a sidebar
// beside the panes; today it's the session button in the bar, whose menu
// switches sessions and holds the session's commands.

import type { Client } from "../../client";
import type { Session } from "../../proto";
import { sessionItems } from "../commands";
import { openFresh } from "../open-fresh";
import { RenameInput, type Renaming } from "./rename-input";

/** The sidebar itself: nothing yet (M108). */
export function SessionSidebar(_: { client: Client }) {
  return null;
}

/** The bar's session button and its menu, or the box renaming it. */
export function SessionButton({
  client,
  session,
  renaming,
  setRenaming,
}: {
  client: Client;
  session: Session;
  renaming: Renaming;
  setRenaming: (r: Renaming) => void;
}) {
  const state = client.state!;
  const sessionMenu = (e: MouseEvent) => {
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    openFresh(client, { clientX: r.left, clientY: r.bottom + 4, preventDefault: () => e.preventDefault() }, () => [
      ...state.sessions.map((s) => ({
        label: `${s.id === session.id ? "✓ " : "    "}${s.name}`,
        run: () => client.selectSession(s.id),
      })),
      "separator",
      ...sessionItems(client, session.id, () => setRenaming({ kind: "session", id: session.id }), { bar: true }),
    ]);
  };
  if (renaming?.kind === "session" && renaming.id === session.id) {
    return (
      <RenameInput
        value={session.name}
        onDone={(name) => {
          setRenaming(null);
          if (name && name !== session.name) client.intent({ op: "rename_session", session: session.id, name });
        }}
      />
    );
  }
  return (
    <button class="session-button" title="Sessions" aria-label={`Session: ${session.name}`} onClick={sessionMenu} onContextMenu={sessionMenu}>
      <span class="session-label">Session</span>
      {session.name}
      {/* M61: the session's thread has messages you haven't read. */}
      {client.thread({ session: session.id })?.unread ? (
        <span class={client.thread({ session: session.id })?.mention ? "session-unread mention" : "session-unread"} title="New in the session thread" />
      ) : null}{" "}
      <span class="caret">▾</span>
    </button>
  );
}
