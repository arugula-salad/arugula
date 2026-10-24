// `/inbox`: the reader's inbox (M171): the PRs and issues on linked repos
// that want something of them, worked out by the platform for the same
// reasons a forge block asks for attention, grouped by reason; and the
// drafts waiting for them to send (M170's list). Mentions and assignments
// on native issues, and mentions in a mirrored item's thread, are entries
// too (M202), and runs that failed (M215). Each entry opens the item (on
// GitHub; a native issue or a run on its page; a thread's mention in the
// thread), and can be marked seen or done.
// New in threads: the threads on issues, runs and the other objects (M196)
// with posts the reader hasn't read, which chat doesn't list and its button
// doesn't count (shared/chat/unread). They're chat's read marks, not
// entries: the page makes them from chat's rooms, live over its stream, and
// Done reads the thread, so the issue's or run's row in its list agrees.
// Read through the viewer check: a private repo's items show only to those
// GitHub lets read it, and a native issue's to its project's members.
// Deploys waiting for the reader's approval (M218) come first, as cards.

import { useEffect, useState } from "preact/hooks";
import type { ChatRoom, Inbox, InboxEntry } from "../../shared/proto.gen";
import { chat, chatStore } from "../../shared/chat-store";
import { counted, threadNews } from "../../shared/chat/unread";
import { issues } from "../../shared/issues/store";
import { platformFetch, refusalOf } from "../../shared/maintenance";
import { useSubscribe } from "../../shared/ui/hooks";
import { DraftList, useMyDrafts } from "../drafts";
import { WaitingDeploys, useWaitingDeploys } from "./deploy";
import { names, roomPath } from "../links";
import { follow, pathOf } from "../router";
import { runs } from "../runs";
import { runTitle } from "../runs-text";
import { session } from "../session";
import { reasonTone, roomTone, toneClass } from "../swarm";
import { toast } from "../toast";

async function call<T>(method: string, path: string, body?: unknown): Promise<T> {
  const r = await platformFetch(path, {
    method,
    credentials: "same-origin",
    headers: body === undefined ? {} : { "Content-Type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const v = (await r.json().catch(() => ({}))) as T & { error?: string; reason?: string };
  if (!r.ok) throw new Error(refusalOf(r.status, v));
  return v;
}

/** The reasons, most pressing first, as headings. `threads` is New in
 * threads, from chat's rooms (`ThreadGroup`); drafts are their own list
 * below. */
const GROUPS: [string, string[], string][] = [
  ["review_requested", ["review_requested"], "Reviews requested"],
  ["changes_requested", ["changes_requested"], "Changes requested"],
  ["checks_failed", ["checks_failed"], "Checks failed"],
  ["run_failed", ["run_failed"], "Runs failed"],
  ["mentioned", ["mentioned", "thread_mention"], "Mentions"],
  ["assigned", ["assigned"], "Assigned to you"],
  ["threads", [], "New in threads"],
  ["done", ["done"], "Merged, closed or ready"],
];

/** Where an entry opens: a native issue on its page, a thread's mention in
 * the thread (both here), anything else on GitHub. */
function linkOf(e: InboxEntry): { href: string; here: boolean } {
  if (e.project && e.item.startsWith("iss_")) return { href: pathOf({ page: "issue", project: e.project, issue: e.item }, names), here: true };
  if (e.project && e.item.startsWith("run_")) return { href: pathOf({ page: "run", project: e.project, run: e.item }, names), here: true };
  if (e.reason === "thread_mention" && e.room) return { href: pathOf({ page: "find", what: "chat", room: e.room }), here: true };
  return { href: e.url, here: false };
}

const STATE: Record<string, string> = { open: "open", closed: "closed", merged: "merged" };

export function InboxPage() {
  const [entries, setEntries] = useState<InboxEntry[] | null>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState<string | null>(null);
  const mine = useMyDrafts();
  const deploys = useWaitingDeploys();
  useSubscribe((fn) => chat.subscribe(fn));
  const load = () =>
    call<Inbox>("GET", "/api/inbox").then(
      (v) => {
        setEntries(v.entries);
        setError("");
      },
      (e: Error) => setError(e.message),
    );
  useEffect(() => void load(), []);
  const mark = async (e: InboxEntry, what: { seen?: boolean; done?: boolean }) => {
    setBusy(`${e.item} ${e.reason}`);
    try {
      await call("POST", `/api/inbox/${encodeURIComponent(e.item)}`, { reason: e.reason, ...what });
      if (what.done) toast("Done");
      await load();
    } catch (err) {
      toast((err as Error).message);
    } finally {
      setBusy(null);
    }
  };
  const shown = (entries ?? []).filter((e) => e.reason !== "draft");
  // Once the entries are in, so a mention's thread isn't here meanwhile.
  const threads = entries !== null && chatStore() ? threadNews(chat.rooms, shown) : [];
  const drafts = mine.drafts ?? [];
  const empty =
    entries !== null && mine.drafts !== null && deploys !== null && shown.length === 0 && threads.length === 0 && drafts.length === 0 && deploys.length === 0;
  const unseen = shown.filter((e) => !e.seen_at).length + threads.length;
  return (
    <section class="platform-section sw-page platform-inbox" data-platform-inbox>
      <h1>Inbox</h1>
      {entries !== null && !empty && (
        <div class="sw-stats" data-inbox-stats>
          <span class={unseen ? "needs" : undefined}>
            <b>{unseen}</b> unseen
          </span>
          <span>
            <b>{shown.length + threads.length}</b> {shown.length + threads.length === 1 ? "item" : "items"}
          </span>
          {!!deploys?.length && (
            <span>
              <b>{deploys.length}</b> {deploys.length === 1 ? "deploy waits" : "deploys wait"}
            </span>
          )}
          {drafts.length > 0 && (
            <span>
              <b>{drafts.length}</b> {drafts.length === 1 ? "draft" : "drafts"}
            </span>
          )}
        </div>
      )}
      {error && <p class="control-error">{error}</p>}
      {entries === null && !error && <p class="dim">Loading…</p>}
      {empty && <p class="dim" data-inbox-empty>Nothing wants you right now.</p>}
      <WaitingDeploys list={deploys} />
      {GROUPS.map(([reason, reasons, heading]) => {
        if (reason === "threads") return <ThreadGroup key={reason} heading={heading} rooms={threads} />;
        const group = shown.filter((e) => reasons.includes(e.reason));
        if (group.length === 0) return null;
        return (
          <div key={reason} data-inbox-group={reason}>
            <h2>
              {heading} <span class="dim">{group.length}</span>
            </h2>
            <ul class="platform-list platform-inbox-list sw-cards">
              {group.map((e) => {
                const key = `${e.item} ${e.reason}`;
                const link = linkOf(e);
                const open = (ev: MouseEvent) => {
                  if (!e.seen_at) void mark(e, { seen: true });
                  if (link.here) follow(ev, link.href);
                };
                const away = link.here ? {} : { target: "_blank", rel: "noopener noreferrer" };
                return (
                  // A card in the swarm's language (../swarm.css): its bar is
                  // why it wants the reader, full while it's unseen.
                  <li key={key} class={`sw-card ${toneClass(reasonTone(e.reason))} ${e.seen_at ? "seen" : "unseen"}`} data-inbox-entry={e.item} data-inbox-reason={e.reason}>
                    <a class="platform-inbox-item" href={link.href} {...away} onClick={open}>
                      <span class="platform-inbox-ref">{e.kind === "run" ? `${e.repo}:` : `${e.repo}#${e.number}`}</span>{" "}
                      <span class="platform-inbox-title">{e.title}</span>
                    </a>
                    <div class="dim platform-inbox-meta">
                      {e.kind === "pr" ? "PR" : e.kind === "run" ? "Run" : "Issue"} · {STATE[e.state] ?? e.state}
                      {e.reason === "thread_mention" ? " · in its thread" : ""}
                      {e.author ? ` · ${e.kind === "run" ? "started by" : "by"} ${e.author}` : ""}
                    </div>
                    <div class="platform-inbox-actions">
                      {!e.seen_at && (
                        <button class="control-linkish" data-inbox-seen={e.item} disabled={busy === key} onClick={() => void mark(e, { seen: true })}>
                          Seen
                        </button>
                      )}
                      <button class="control-linkish" data-inbox-done={e.item} disabled={busy === key} onClick={() => void mark(e, { done: true })}>
                        Done
                      </button>
                    </div>
                  </li>
                );
              })}
            </ul>
          </div>
        );
      })}
      {(drafts.length > 0 || mine.error) && (
        <div data-inbox-group="draft">
          <h2>
            Drafts waiting for you <span class="dim">{drafts.length}</span>
          </h2>
          {mine.error && <p class="dim">{mine.error}</p>}
          <DraftList drafts={drafts} nameOf={() => "you"} onChange={() => void mine.reload().then(load)} />
        </div>
      )}
    </section>
  );
}

/** What each kind of object's thread is on, by its id's prefix. */
function kindOf(r: ChatRoom): string {
  const t = r.target ?? "";
  if (t.startsWith("iss_")) return "Issue";
  if (t.startsWith("run_")) return "Run";
  if (t.startsWith("hw_")) return "Workspace";
  return r.name.startsWith("PR ") ? "PR" : "Issue";
}

/** How many titles the page asks for at once, for threads it hasn't read
 * the issue or run of. The rest say their room's name. */
const TITLES = 25;
const asked = new Set<string>();

/** New in threads: a card per object whose thread has something unread,
 * opening the object (an issue or a run on its page, anything else in
 * chat), whose page reads the thread. Done reads it from here. */
function ThreadGroup({ heading, rooms }: { heading: string; rooms: ChatRoom[] }) {
  useSubscribe((fn) => issues.subscribe(fn));
  useSubscribe((fn) => runs.subscribe(fn));
  useSubscribe((fn) => session.subscribe(fn));
  useEffect(() => {
    let n = 0;
    for (const r of rooms) {
      const t = r.target ?? "";
      if (asked.has(t) || n >= TITLES) continue;
      if (t.startsWith("iss_") && !issues.known(t)) void issues.get(t).catch(() => {});
      else if (t.startsWith("run_") && !runs.known(t)) void runs.get(t).catch(() => {});
      else continue;
      asked.add(t);
      n++;
    }
  }, [rooms.map((r) => r.id).join()]);
  if (rooms.length === 0) return null;
  return (
    <div data-inbox-group="threads">
      <h2>
        {heading} <span class="dim">{rooms.length}</span>
      </h2>
      <ul class="platform-list platform-inbox-list sw-cards">
        {rooms.map((r) => {
          const t = r.target ?? "";
          const href = roomPath(r.id);
          const project = session.project(r.project)?.name ?? "";
          const issue = t.startsWith("iss_") ? issues.known(t) : undefined;
          const run = t.startsWith("run_") ? runs.known(t) : undefined;
          const ref = issue ? `${project}#${issue.number}` : run ? `${project}:` : project ? `${project} ·` : "";
          const title = issue?.title ?? (run ? runTitle(run.op) : r.name);
          const n = counted(r);
          return (
            // As an entry's card: its bar is chat's colour for the room.
            <li key={r.id} class={`sw-card ${toneClass(roomTone(n, r.mentions))} unseen`} data-inbox-entry={t} data-inbox-reason="thread" data-inbox-thread={r.id}>
              <a class="platform-inbox-item" href={href} onClick={(ev) => follow(ev, href)}>
                {ref && <span class="platform-inbox-ref">{ref}</span>} <span class="platform-inbox-title">{title}</span>
              </a>
              <div class="dim platform-inbox-meta">
                {kindOf(r)} · {n} new in its thread{r.mentions ? ", mentioning you" : ""}
              </div>
              <div class="platform-inbox-actions">
                <button class="control-linkish" title="Mark its thread read" data-inbox-thread-done={r.id} onClick={() => void chat.read(r.id, r.last)}>
                  Done
                </button>
              </div>
            </li>
          );
        })}
      </ul>
    </div>
  );
}
