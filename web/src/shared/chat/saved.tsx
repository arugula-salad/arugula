// Mark as unread and saved for later (#116), in the page: a message's menu
// (beside the hover toolbar's other tools) and the Saved view beside
// Activity. What's saved is control's, per account, and reaches this page's
// other copies over the live stream (../chat-store).

import { useEffect, useState } from "preact/hooks";
import { chat } from "../chat-store";
import { usePlatformApi } from "../platform-api";
import { useSubscribe } from "../ui/hooks";
import { Markup } from "../ui/markup";
import type { ChatSavedItem } from "../proto";
import { authorLabel } from "./agents";
import { AuthorAvatar } from "./room";
import { ViewHead, when, whereOf } from "./find";

export const HOUR = 3_600_000;

/** When "tomorrow" is for a reminder: nine in the morning, the next day,
 * where the reader is. */
export function tomorrowMorning(now = new Date()): number {
  const d = new Date(now);
  d.setDate(d.getDate() + 1);
  d.setHours(9, 0, 0, 0);
  return d.getTime();
}

function reminderNote(i: ChatSavedItem): string | null {
  if (i.remind_at === null) return null;
  return i.reminded ? `Reminded ${when(i.remind_at)}` : `Reminder ${when(i.remind_at)}`;
}

function SavedRow({ item, onError }: { item: ChatSavedItem; onError: (e: string) => void }) {
  const api = usePlatformApi();
  const r = chat.room(item.room);
  const m = item.message;
  const done = item.done !== null;
  const act = (f: Promise<void>) => f.catch((e: Error) => onError(e.message));
  const note = done ? null : reminderNote(item);
  return (
    <li class={done ? "saved-item done" : "saved-item"} data-saved={`${item.room}/${item.seq}`}>
      <button class="chat-hit" data-chat-hit={item.seq} data-hit-room={item.room} onClick={() => api.nav.openRoom(item.room, item.seq)}>
        <span class="chat-hit-where">
          <span>{r ? whereOf(r, api) : "a room"}</span>
          <time>{when(m.at)}</time>
        </span>
        <span class="chat-hit-body">
          <AuthorAvatar m={m} />
          <span class={m.agent ? "chat-hit-text by-agent" : "chat-hit-text"}>
            <b>{authorLabel(m)}</b>
            {m.text ? <Markup text={m.text} /> : <p class="thread-text">{m.quote}</p>}
          </span>
        </span>
      </button>
      <div class="saved-actions">
        {note && <span class="saved-note">{note}</span>}
        {!done && (
          <>
            <button data-saved-remind="hour" onClick={() => act(chat.save(item.room, item.seq, Date.now() + HOUR))}>
              Remind in 1 h
            </button>
            <button data-saved-remind="tomorrow" onClick={() => act(chat.save(item.room, item.seq, tomorrowMorning()))}>
              Remind tomorrow
            </button>
          </>
        )}
        <button data-saved-done onClick={() => act(chat.savedDone(item.room, item.seq, !done))}>
          {done ? "Undo" : "Done"}
        </button>
        <button data-saved-remove onClick={() => act(chat.unsave(item.room, item.seq))}>
          Remove
        </button>
      </div>
    </li>
  );
}

/** Saved for later: the messages this person saved, to do first, then done
 * ones, each with its room, a link to it and what to do with it. */
export function SavedView({ phone }: { phone: boolean }) {
  useSubscribe((fn) => chat.subscribe(fn));
  const [err, setErr] = useState("");
  useEffect(() => {
    void chat.loadSaved();
  }, []);
  const items = chat.saved;
  const todo = items?.filter((i) => i.done === null) ?? [];
  const finished = items?.filter((i) => i.done !== null) ?? [];
  return (
    <section class="chat-thread chat-view" aria-label="Saved" data-chat-saved-view>
      <ViewHead title="Saved for later" sub={items ? `${todo.length} to do` : undefined} phone={phone} />
      <div class="thread-list">
        {items === null && <p class="thread-empty">Loading…</p>}
        {items?.length === 0 && <p class="thread-empty">Nothing saved. Save a message from its menu to come back to it, with a reminder if you like. Only you see this.</p>}
        {err && <p class="thread-error">{err}</p>}
        {todo.length > 0 && (
          <ul class="chat-hits saved-list">
            {todo.map((i) => (
              <SavedRow key={`${i.room}/${i.seq}`} item={i} onError={setErr} />
            ))}
          </ul>
        )}
        {finished.length > 0 && <h3 class="activity-head">Done</h3>}
        {finished.length > 0 && (
          <ul class="chat-hits saved-list">
            {finished.map((i) => (
              <SavedRow key={`${i.room}/${i.seq}`} item={i} onError={setErr} />
            ))}
          </ul>
        )}
      </div>
    </section>
  );
}
