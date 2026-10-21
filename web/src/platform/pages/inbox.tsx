// `/inbox`: the reader's inbox (M171): the PRs and issues on linked repos
// that want something of them, worked out by the platform for the same
// reasons a forge block asks for attention, grouped by reason; and the
// drafts waiting for them to send (M170's list). Each entry opens the item
// on GitHub, and can be marked seen or done. Read through the viewer check:
// a private repo's items show only to those GitHub lets read it.

import { useEffect, useState } from "preact/hooks";
import type { Inbox, InboxEntry } from "../../shared/proto.gen";
import { platformFetch, refusalOf } from "../../shared/maintenance";
import { DraftList, useMyDrafts } from "../drafts";
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
const GROUPS: [string, string][] = [
  ["review_requested", "Reviews requested"],
  ["changes_requested", "Changes requested"],
  ["checks_failed", "Checks failed"],
  ["mentioned", "Mentions"],
  ["assigned", "Assigned to you"],
  ["done", "Merged, closed or ready"],
];

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
      {empty && <p class="dim" data-inbox-empty>Nothing wants you on your linked repos right now.</p>}
      {GROUPS.map(([reason, heading]) => {
        const group = shown.filter((e) => e.reason === reason);
        if (group.length === 0) return null;
        return (
          <div key={reason} data-inbox-group={reason}>
            <h2>
              {heading} <span class="dim">{group.length}</span>
            </h2>
            <ul class="platform-list platform-inbox-list">
              {group.map((e) => {
                const key = `${e.item} ${e.reason}`;
                return (
                  <li key={key} class={e.seen_at ? "seen" : "unseen"} data-inbox-entry={e.item}>
                    <a class="platform-inbox-item" href={e.url} target="_blank" rel="noopener noreferrer" onClick={() => !e.seen_at && void mark(e, { seen: true })}>
                      <span class="platform-inbox-ref">
                        {e.repo}#{e.number}
                      </span>{" "}
                      <span class="platform-inbox-title">{e.title}</span>
                    </a>
                    <div class="dim platform-inbox-meta">
                      {e.kind === "pr" ? "PR" : "Issue"} · {STATE[e.state] ?? e.state}
                      {e.author ? ` · by ${e.author}` : ""}
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
