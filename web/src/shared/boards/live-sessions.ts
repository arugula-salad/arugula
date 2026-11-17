// A board's rows' sessions, live (Arugula #1117): the project's sessions
// by card (row-sessions.ts draws a row's), read once a minute while the
// page is seen, and soon after a hosted session on one of the board's
// cards says its state moved (working, needs input, done): the platform
// pushes that as the session's live event (`chat.onObject(ses)`), as the
// Sessions page heard it. A session a row holds itself reads the board
// again instead. A computer's session or agent block has no such push to
// the platform's page: it waits for the minute's read.

import { useEffect, useRef, useState } from "preact/hooks";
import type { BoardRow } from "../proto";
import { chat } from "../chat-store";
import { BY_CARD_EVERY_MS, sessionsByCard, type SessionsByCard } from "../project-sessions";
import { pushedSessions } from "./row-sessions";

/** A pushed change reads the by-card list again no sooner than this after
 * the last push's read: with the minute's, well under the hub's 600 an hour. */
export const BY_CARD_SOON_MS = 10_000;

/** The board's own read again after a row's session's push: a burst of
 * them (a turn's steps) reads it once. */
const BOARD_SOON_MS = 300;

/** The project's sessions by card while `cards` (and the board is shown),
 * kept live; null when off, before it answers, or where it can't. While
 * `shown`, a push for a session a row holds calls `boardChanged`. */
export function useLiveRowSessions(project: string | undefined, rows: BoardRow[], { shown, cards }: { shown: boolean; cards: boolean }, boardChanged: () => void): SessionsByCard | null {
  const [v, setV] = useState<SessionsByCard | null>(null);
  /** Read the list again for a push: now, or BY_CARD_SOON_MS after the last push's read. */
  const soon = useRef<() => void>(() => {});
  const on = cards && shown;
  useEffect(() => {
    setV(null);
    soon.current = () => {};
    if (!on || !project) return;
    let live = true;
    let last = 0;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const read = () =>
      sessionsByCard(project).then(
        (x) => live && setV(x),
        () => {},
      );
    soon.current = () => {
      if (timer !== undefined) return;
      timer = setTimeout(
        () => {
          timer = undefined;
          last = Date.now();
          void read();
        },
        Math.max(0, last + BY_CARD_SOON_MS - Date.now()),
      );
    };
    void read();
    const t = setInterval(() => document.visibilityState === "visible" && void read(), BY_CARD_EVERY_MS);
    return () => {
      live = false;
      clearTimeout(timer);
      clearInterval(t);
      soon.current = () => {};
    };
  }, [project, on]);

  const pushed = pushedSessions(rows, on ? v : null);
  const changed = useRef(boardChanged);
  changed.current = boardChanged;
  const key = shown ? JSON.stringify(pushed) : "";
  useEffect(() => {
    if (!key) return;
    const { board, cards: listed } = JSON.parse(key) as typeof pushed;
    const mine = new Set(board);
    let timer: ReturnType<typeof setTimeout> | undefined;
    const offs = [...new Set([...board, ...listed])].map((ses) =>
      chat.onObject(ses, () => {
        if (!mine.has(ses)) return soon.current();
        if (timer === undefined) timer = setTimeout(() => ((timer = undefined), changed.current()), BOARD_SOON_MS);
      }),
    );
    return () => {
      clearTimeout(timer);
      offs.forEach((off) => off());
    };
  }, [key]);
  return v;
}
