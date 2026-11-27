// The count on the bar's Inbox (UI audit #170), and the switcher's per
// project: the one Inbox's, as the server says it (Arugula #1430, #1433).
// `GET /api/inbox`'s `count` and `by_project` are shown as they are:
// nothing is added to them here, and nothing is counted from the list, so
// an alert (checks or a run failed) shows in the list without counting.
// The Inbox is read when the bar comes up, when the reader comes back to
// the window, whenever the inbox page reads all of it (it hands it here),
// and again when chat's stream says it changed (its `inbox` event, whose
// count shows at once).

import { useEffect, useState } from "preact/hooks";
import type { Inbox, InboxItem } from "../shared/proto.gen";
import { chat } from "../shared/chat-store";
import { platformFetch, refusalOf } from "../shared/maintenance";
import { needYouText } from "../shared/needs-you";
import { useSubscribe } from "../shared/ui/hooks";
import { inboxPath } from "./scope";
import { session } from "./session";

/** The whole Inbox as last read, or null before it is. */
let read: Inbox | null = null;
const listeners = new Set<() => void>();
const emit = () => listeners.forEach((fn) => fn());

/** What the inbox page just read: all of it, or one project's (`scope`),
 * which isn't the bar's to keep. */
export function noteInbox(v: Inbox, scope?: string) {
  if (scope) return;
  read = v;
  emit();
}

/** Whether an item waits on a teammate, not the reader (Arugula #1393):
 * `for` someone else. Never counted; folded as "N asks for Jo ›". */
export const forTeammate = (i: InboxItem, me: string | null | undefined): boolean => !!i.for && !!me && i.for !== me;

/** What an item is, in a few words, under its title. A kind this doesn't
 * know says nothing: its title says it. */
export const kindWords: Record<string, string> = {
  agent_ask: "An agent asks",
  permission: "An agent asks for a permission",
  agent_failed: "An agent failed",
  proposal: "Your agent proposes",
  board_ask: "A board asks",
  deploy_approval: "Waits for your approval",
  checks_failed: "Checks failed",
  run_failed: "Run failed",
  changes_requested: "Changes requested",
  review_request: "Review requested",
  mention: "Mentions you",
  assigned: "Assigned to you",
  dm: "Message",
  thread_news: "New in its thread",
};

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

/** Whether an item is in the reader's count, by the server's rule. */
export const counted = (i: InboxItem, me: string | null | undefined): boolean => i.counts && !i.done_at && !forTeammate(i, me);

/** `GET /api/inbox` for `project`, or all of it. */
export async function readInbox(project?: string): Promise<Inbox> {
  const r = await platformFetch(inboxPath(project), { credentials: "same-origin" });
  const v = (await r.json().catch(() => ({}))) as Inbox & { error?: string; reason?: string };
  if (!r.ok) throw new Error(refusalOf(r.status, v));
  return v;
}

/** Mark an item seen or done (`POST /api/inbox/{id}`), then read the
 * bar's count again. */
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
      (v) => noteInbox(v),
      // The count waits for the next read; the page says what went wrong.
      () => {},
    )
    .finally(() => (loading = null));
  return loading;
}

/** What the stream said last, so each `inbox` event is taken once. */
let heard = 0;
chat.subscribe(() => {
  const ev = chat.inbox;
  if (!ev || ev.heard === heard) return;
  heard = ev.heard;
  // Its count now; its items with the next read.
  read = { entries: [], items: [], handled: 0, ...read, count: ev.count, by_project: ev.by_project };
  emit();
  void load();
});

/** The Inbox's counts as last read, for what isn't drawn (Go to…'s
 * projects); `subscribeInbox` says when they change. */
export const inboxCounts = (): { all: number; by: Map<string, number> } => ({ all: read?.count ?? 0, by: new Map(Object.entries(read?.by_project ?? {})) });
export function subscribeInbox(fn: () => void): () => void {
  listeners.add(fn);
  return () => void listeners.delete(fn);
}

/** The Inbox's count for all projects and by project (Arugula #989's
 * switcher), kept current while it's drawn; empty until it's read. */
export function useInboxByProject(): { all: number; by: Map<string, number> } {
  useSubscribe((fn) => {
    listeners.add(fn);
    return () => void listeners.delete(fn);
  });
  useEffect(() => void (read ?? load()), []);
  return { all: read?.count ?? 0, by: new Map(Object.entries(read?.by_project ?? {})) };
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

/** What the Inbox's count says, read and kept current while it's drawn:
 * the server's count across the reader's projects (the Inbox is one
 * place, Arugula #1433), how many of its items are deploys waiting, and in
 * words. */
export function useInboxCount(): { n: number; waiting: number; title: string } {
  useSubscribe((fn) => {
    listeners.add(fn);
    return () => void listeners.delete(fn);
  });
  // Once signed in: the bar is drawn while the page signs in, and a call
  // before then would race the sign-in.
  const ready = session.state === "ready";
  useEffect(() => {
    if (!ready) return;
    void load();
    const back = () => document.visibilityState === "visible" && void load();
    document.addEventListener("visibilitychange", back);
    return () => document.removeEventListener("visibilitychange", back);
  }, [ready]);
  const n = read?.count ?? 0;
  const me = session.me?.account;
  const waiting = (read?.items ?? []).filter((i) => i.kind === "deploy_approval" && counted(i, me)).length;
  const said = [waiting ? `${waiting} ${waiting === 1 ? "deploy waits" : "deploys wait"} for your approval` : "", n ? needYouText(n) : ""];
  return { n, waiting, title: said.filter(Boolean).join("; ") };
}
