// A board's view for the swarm's focus (#466), read as the person looking
// and read again when the platform says it changed: a row that moves lane
// moves its worker with it. The same reads as the board page's.

import { useEffect, useState } from "preact/hooks";
import type { BoardView } from "../../shared/proto";
import { boards } from "../../shared/boards/store";
import { chat } from "../../shared/chat-store";

export interface BoardRead {
  view: BoardView | null;
  /** Why it can't be read, when it can't. */
  missing: string | null;
}

/** Board `id`'s view, live; nothing for no board. */
export function useBoardView(id: string | null): BoardRead {
  const [read, setRead] = useState<BoardRead>({ view: null, missing: null });
  useEffect(() => {
    setRead({ view: null, missing: null });
    if (!id) return;
    let n = 0;
    const again = () => {
      const mine = ++n;
      boards.get(id).then(
        (view) => mine === n && setRead({ view, missing: null }),
        (e: Error) => mine === n && setRead((r) => ({ view: r.view, missing: e.message })),
      );
    };
    again();
    // The thread is what the platform pushes a change on: open it so there is one.
    boards.thread(id).catch(() => {});
    const off = chat.onObject(id, again);
    const back = () => document.visibilityState === "visible" && again();
    document.addEventListener("visibilitychange", back);
    return () => {
      n++;
      off();
      document.removeEventListener("visibilitychange", back);
    };
  }, [id]);
  return read;
}
