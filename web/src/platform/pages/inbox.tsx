// `/inbox`: the reader's inbox (M171): the PRs and issues on linked repos
// that want something of them, worked out by the platform for the same
// reasons a forge block asks for attention, grouped by reason; and the
// drafts waiting for them to send (M170's list). Mentions and assignments
// on native issues, and mentions in a mirrored item's thread, are entries
// too (M202), and runs that failed (M215). Each entry opens the item (on
// GitHub; a native issue or a run on its page; a thread's mention in the
// thread), and can be marked seen or done.
// Read through the viewer check: a private repo's items show only to those
// GitHub lets read it, and a native issue's to its project's members.

import { useEffect, useState } from "preact/hooks";
import type { Inbox, InboxEntry } from "../../shared/proto.gen";
import { platformFetch, refusalOf } from "../../shared/maintenance";
import { DraftList, useMyDrafts } from "../drafts";
import { names } from "../links";
import { follow, pathOf } from "../router";
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

/** The reasons, most pressing first, as headings. Drafts are their own
 * list below. */
const GROUPS: [string, string[], string][] = [
  ["review_requested", ["review_requested"], "Reviews requested"],
  ["changes_requested", ["changes_requested"], "Changes requested"],
  ["checks_failed", ["checks_failed"], "Checks failed"],
  ["run_failed", ["run_failed"], "Runs failed"],
  ["mentioned", ["mentioned", "thread_mention"], "Mentions"],
  ["assigned", ["assigned"], "Assigned to you"],
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
  const drafts = mine.drafts ?? [];
  const empty = entries !== null && mine.drafts !== null && shown.length === 0 && drafts.length === 0;
  return (
    <section class="platform-section platform-inbox" data-platform-inbox>
      <h1>Inbox</h1>
      {error && <p class="control-error">{error}</p>}
      {entries === null && !error && <p class="dim">Loading…</p>}
      {empty && <p class="dim" data-inbox-empty>Nothing wants you right now.</p>}
      {GROUPS.map(([reason, reasons, heading]) => {
        const group = shown.filter((e) => reasons.includes(e.reason));
        if (group.length === 0) return null;
        return (
          <div key={reason} data-inbox-group={reason}>
            <h2>
              {heading} <span class="dim">{group.length}</span>
            </h2>
            <ul class="platform-list platform-inbox-list">
              {group.map((e) => {
                const key = `${e.item} ${e.reason}`;
                const link = linkOf(e);
                const open = (ev: MouseEvent) => {
                  if (!e.seen_at) void mark(e, { seen: true });
                  if (link.here) follow(ev, link.href);
                };
                const away = link.here ? {} : { target: "_blank", rel: "noopener noreferrer" };
                return (
                  <li key={key} class={e.seen_at ? "seen" : "unseen"} data-inbox-entry={e.item} data-inbox-reason={e.reason}>
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
