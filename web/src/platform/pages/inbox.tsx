// `/inbox`: the reader's one Inbox (Arugula #1430, #1433), as the server
// lists it: `GET /api/inbox`'s `items`, in its order and groups (Decide,
// then Names you, then Chat), and its `count`, shown as it is. Each item
// opens where its subject is (on GitHub, or here: a native issue or a run
// on its page, a thread or a DM in chat), which marks it seen, and can be
// dismissed (Dismiss, as control's cards say it: UI audit #172), which
// marks it done; a thread's or a DM's done reads it. An item that waits on
// a teammate (`for` them, Arugula #1393) is folded under "N asks for Jo ›",
// never counted. A kind the page doesn't know shows by its title.
// Deploys waiting for the reader's approval (M218) come first, as their
// cards, so they're answered here; and the drafts waiting for the reader
// to send (M170's list) last.

import { Card } from "../../shared/ui/card";
import { useEffect, useState } from "preact/hooks";
import type { Inbox, InboxItem } from "../../shared/proto.gen";
import { useSubscribe } from "../../shared/ui/hooks";
import { PageHeader } from "../../shared/ui/page-header";
import { chat } from "../../shared/chat-store";
import { DraftList, useMyDrafts } from "../drafts";
import { asksFor, byTeammate, forTeammate, kindWords, markItem, memberName, noteInbox, readInbox } from "../inbox-count";
import { currentProject, onCurrentProject } from "../current-project";
import { scopeOf } from "../scope";
import { ScopeToggle } from "../scope-toggle";
import { WaitingDeploys, useWaitingDeploys } from "./deploy";
import { follow } from "../router";
import { session } from "../session";
import { itemTone } from "../swarm";
import { toast } from "../toast";

/** The server's groups, in its order, as headings. `ready` (a finished
 * turn, never counted) isn't listed. */
const GROUPS: [string, string][] = [
  ["decide", "Decide"],
  ["named", "Names you"],
  ["chat", "Chat"],
];

/** An item's title, its reference apart where it starts with one
 * (`acme/app#7 Retry…`, `#12 Flaky…`), as the list has always shown it. */
function titled(title: string): { ref: string | null; title: string } {
  const m = /^(\S*#\d+) (.+)$/.exec(title);
  return m ? { ref: m[1], title: m[2] } : { ref: null, title };
}

/** Where an item opens: a path on this page, or away (GitHub). */
const here = (url: string) => url.startsWith("/");

export function InboxPage({ all }: { all?: boolean }) {
  const [inbox, setInbox] = useState<Inbox | null>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState<string | null>(null);
  const [unfolded, setUnfolded] = useState<ReadonlySet<string>>(new Set());
  const mine = useMyDrafts();
  const deploys = useWaitingDeploys();
  useSubscribe((fn) => session.subscribe(fn));
  useSubscribe((fn) => onCurrentProject(fn));
  // The stream says when the Inbox changed: read it again then.
  useSubscribe((fn) => chat.subscribe(fn));
  const said = chat.inbox?.heard ?? 0;
  // The current project's, unless All (Arugula #881): the platform filters.
  const scope = scopeOf({ page: "inbox", all: all || undefined }, currentProject(), (id) => !!session.project(id));
  const load = () =>
    readInbox(scope).then(
      (v) => {
        setInbox(v);
        // The bar's count says the same (UI audit #170).
        noteInbox(v, scope);
        setError("");
      },
      (e: Error) => setError(e.message),
    );
  useEffect(() => {
    setInbox(null);
  }, [scope]);
  useEffect(() => {
    void load();
  }, [scope, said]);
  const mark = async (i: InboxItem, what: { seen?: boolean; done?: boolean }) => {
    setBusy(i.id);
    try {
      await markItem(i.id, what);
      if (what.done) toast("Dismissed");
      await load();
    } catch (err) {
      toast((err as Error).message);
    } finally {
      setBusy(null);
    }
  };
  const me = session.me?.account;
  const items = (inbox?.items ?? []).filter((i) => i.group !== "ready" && !i.done_at);
  // A deploy the list names is its card, answered here, not a row too.
  const cards = (deploys ?? []).filter((d) => items.some((i) => i.kind === "deploy_approval" && i.subject.id === d.id));
  const rows = items.filter((i) => !(i.kind === "deploy_approval" && cards.some((d) => d.id === i.subject.id)));
  const drafts = mine.drafts ?? [];
  const empty = inbox !== null && mine.drafts !== null && items.length === 0 && drafts.length === 0;
  const count = inbox?.count ?? 0;
  const row = (i: InboxItem) => {
    const seen = !!i.seen_at;
    const { ref, title } = titled(i.title);
    const at = here(i.subject.url);
    const open = (ev: MouseEvent) => {
      if (!seen) void mark(i, { seen: true });
      if (at) follow(ev, i.subject.url);
    };
    const away = at ? {} : { target: "_blank", rel: "noopener noreferrer" };
    const thread = i.kind === "thread_news" ? i.subject.room : undefined;
    const meta = [kindWords[i.kind], i.from.name ? `from ${i.from.name}` : "", i.escalated ? "no agent answered it" : ""].filter(Boolean).join(" · ");
    return (
      // A card in the swarm's language (../swarm.css): its bar is what it
      // wants of the reader, full while it's unseen.
      <Card as="li" key={i.id} tone={itemTone(i.kind)} seen={seen} class={seen ? undefined : "unseen"} data-inbox-item={i.id} data-inbox-kind={i.kind} data-inbox-entry={i.subject.id} data-inbox-thread={thread}>
        <a class="platform-inbox-item" href={i.subject.url} {...away} onClick={open}>
          {ref && <span class="platform-inbox-ref">{ref}</span>} <span class="platform-inbox-title">{title}</span>
        </a>
        {(meta || i.snippet) && (
          <div class="dim platform-inbox-meta">
            {meta}
            {meta && i.snippet ? ": " : ""}
            {i.snippet}
          </div>
        )}
        <div class="platform-inbox-actions">
          <button class="secondary small" data-inbox-done={i.id} data-inbox-thread-done={thread} disabled={busy === i.id} onClick={() => void mark(i, { done: true })}>
            Dismiss
          </button>
        </div>
      </Card>
    );
  };
  return (
    <section class="platform-section sw-page platform-inbox" data-platform-inbox>
      <PageHeader
        title="Inbox"
        actions={<ScopeToggle r={{ page: "inbox" }} scoped={!!scope} all={!!all} />}
        stats={
          inbox !== null &&
          !empty && (
            <div class="sw-stats" data-inbox-stats>
              <span class={count ? "needs" : undefined} data-inbox-count={count}>
                <b>{count}</b> {count === 1 ? "needs" : "need"} you
              </span>
              <span>
                <b>{items.length}</b> {items.length === 1 ? "item" : "items"}
              </span>
              {drafts.length > 0 && (
                <span>
                  <b>{drafts.length}</b> {drafts.length === 1 ? "draft" : "drafts"}
                </span>
              )}
            </div>
          )
        }
      />
      {error && <p class="control-error">{error}</p>}
      {inbox === null && !error && <p class="dim">Loading…</p>}
      {empty && <p class="dim" data-inbox-empty>Nothing wants you right now.</p>}
      <WaitingDeploys list={cards} />
      {GROUPS.map(([group, heading]) => {
        const list = rows.filter((i) => i.group === group);
        const yours = list.filter((i) => !forTeammate(i, me));
        const folds = byTeammate(list, me);
        if (list.length === 0) return null;
        return (
          <div key={group} data-inbox-group={group}>
            <h2>
              {heading} <span class="dim">{yours.length}</span>
            </h2>
            {yours.length > 0 && <ul class="platform-list platform-inbox-list card-list">{yours.map(row)}</ul>}
            {folds.map(([who, theirs]) => {
              const key = `${group} ${who}`;
              const open = unfolded.has(key);
              return (
                <div key={key} data-inbox-for={who}>
                  <p>
                    <button type="button" class="link" aria-expanded={open} onClick={() => setUnfolded((s) => (open ? new Set([...s].filter((k) => k !== key)) : new Set([...s, key])))}>
                      {asksFor(theirs.length, memberName(who))}
                    </button>
                  </p>
                  {open && <ul class="platform-list platform-inbox-list card-list">{theirs.map(row)}</ul>}
                </div>
              );
            })}
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
