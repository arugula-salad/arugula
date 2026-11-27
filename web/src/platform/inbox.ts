// The one Inbox, as the server says it (Arugula #1430, #1433): `GET
// /api/inbox`'s `items`, `count` and `by_project`, kept here for the rail's
// badge (the only count across projects), its project marks, Go to… and
// the Inbox place. Nothing is counted here: the numbers are the server's,
// so an alert shows in the list without counting. It's read when the rail
// comes up, when the reader comes back to the window, and again when chat's
// stream says it changed (its `inbox` event, whose counts show at once).

import { useEffect, useState } from "preact/hooks";
import type { Inbox, InboxItem } from "../shared/proto.gen";
import { chat } from "../shared/chat-store";
import { platformFetch, refusalOf } from "../shared/maintenance";
import { useSubscribe } from "../shared/ui/hooks";
import { inboxPath } from "./scope";
import { session } from "./session";

/** The whole Inbox as last read, or null before it is. */
let read: Inbox | null = null;
/** Why the last read failed, until one doesn't. */
let failed = "";
const listeners = new Set<() => void>();
const emit = () => listeners.forEach((fn) => fn());
const listen = (fn: () => void) => {
  listeners.add(fn);
  return () => void listeners.delete(fn);
};

/** Whether an item waits on a teammate, not the reader (Arugula #1393):
 * `for` someone else. Never counted; folded as "N asks for Jo ›". */
export const forTeammate = (i: InboxItem, me: string | null | undefined): boolean => !!i.for && !!me && i.for !== me;

/** A teammate's name, as their teams list it, else their account. */
export function memberName(account: string): string {
  for (const p of session.projects) for (const m of p.team?.members ?? []) if (m.account === account && m.name) return m.name;
  return account;
}

/** Items `for` teammates, by whom: the most first, each in the list's
 * order. */
export function byTeammate(items: InboxItem[], me: string | null | undefined): [string, InboxItem[]][] {
  const by = new Map<string, InboxItem[]>();
  for (const i of items) if (forTeammate(i, me)) by.set(i.for!, [...(by.get(i.for!) ?? []), i]);
  return [...by].sort((a, b) => b[1].length - a[1].length);
}

/** "1 ask for Jo ›", "3 asks for Jo ›". */
export const asksFor = (n: number, name: string) => `${n} ${n === 1 ? "ask" : "asks"} for ${name} ›`;

/** `GET /api/inbox` for `project`, or all of it. */
export async function readInbox(project?: string): Promise<Inbox> {
  const r = await platformFetch(inboxPath(project), { credentials: "same-origin" });
  const v = (await r.json().catch(() => ({}))) as Inbox & { error?: string; reason?: string };
  if (!r.ok) throw new Error(refusalOf(r.status, v));
  return v;
}

/** Mark an item seen or done (`POST /api/inbox/{id}`), then read the
 * Inbox again. */
export async function markItem(id: string, what: { seen?: boolean; done?: boolean }): Promise<void> {
  const r = await platformFetch(`/api/inbox/${encodeURIComponent(id)}`, {
    method: "POST",
    credentials: "same-origin",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(what),
  });
  if (!r.ok) throw new Error(refusalOf(r.status, (await r.json().catch(() => ({}))) as { error?: string }));
  void load();
}

/** Answer an approval or a to-do (`POST /api/inbox/{id}` with its
 * `choice`, and a to-do's note), as its staff card's buttons do; throws
 * the server's words. Then read the bar's count again. */
export async function answerItem(id: string, choice: string, note?: string): Promise<void> {
  const r = await platformFetch(`/api/inbox/${encodeURIComponent(id)}`, {
    method: "POST",
    credentials: "same-origin",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(note?.trim() ? { choice, note: note.trim() } : { choice }),
  });
  if (!r.ok) throw new Error(refusalOf(r.status, (await r.json().catch(() => ({}))) as { error?: string }));
  void load();
}

let loading: Promise<void> | null = null;
function load(): Promise<void> {
  loading ??= readInbox()
    .then(
      (v) => {
        read = v;
        failed = "";
      },
      (e: Error) => void (failed = e.message),
    )
    .then(emit)
    .finally(() => (loading = null));
  return loading;
}

/** What the stream said last, so each `inbox` event is taken once. */
let heard = 0;
chat.subscribe(() => {
  const ev = chat.inbox;
  if (!ev || ev.heard === heard) return;
  heard = ev.heard;
  // Its counts now; its items with the next read.
  read = { entries: [], items: [], handled: 0, ...read, count: ev.count, by_project: ev.by_project };
  emit();
  void load();
});

/** The Inbox's counts as last read, for what isn't drawn (Go to…'s
 * projects); `subscribeInbox` says when they change. */
export const inboxCounts = (): { all: number; by: Map<string, number> } => ({ all: read?.count ?? 0, by: new Map(Object.entries(read?.by_project ?? {})) });
export const subscribeInbox = listen;

/** The whole Inbox, kept current while it's drawn: read once signed in
 * (the rail is drawn while the page signs in, and a call before then would
 * race it), and again when the reader comes back to the window. Null
 * until read; `error`, why the last read failed. */
export function useInbox(): { inbox: Inbox | null; error: string } {
  useSubscribe(listen);
  useSubscribe((fn) => session.subscribe(fn));
  const ready = session.state === "ready";
  useEffect(() => {
    if (!ready) return;
    void load();
    const back = () => document.visibilityState === "visible" && void load();
    document.addEventListener("visibilitychange", back);
    return () => document.removeEventListener("visibilitychange", back);
  }, [ready]);
  return { inbox: read, error: failed };
}

/** The rail's badge and its project marks: the server's `count` and
 * `by_project`, as they are. */
export function useInboxCounts(): { all: number; by: Map<string, number> } {
  useInbox();
  return inboxCounts();
}

/** One project's Inbox (the Overview's Needs you), read now, when `again`
 * changes, and when the stream says the Inbox changed; null until read. A
 * read that fails leaves the last one. */
export function useProjectInbox(project: string, again: string): Inbox | null {
  useSubscribe((fn) => chat.subscribe(fn));
  const said = chat.inbox?.heard ?? 0;
  const [v, setV] = useState<{ project: string; inbox: Inbox } | null>(null);
  useEffect(() => {
    let live = true;
    readInbox(project).then(
      (inbox) => live && setV({ project, inbox }),
      () => {},
    );
    return () => void (live = false);
  }, [project, again, said]);
  return v?.project === project ? v.inbox : null;
}
