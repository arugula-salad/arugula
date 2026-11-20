// The Overview's briefing line (Arugula #1277), under its stats: what
// happened in the project since the reader last looked, as "since 18:04
// yesterday: 4 merged · 1 went red · 2 wait on your review". The page
// counts it from the event log itself (`/api/projects/{p}/events`, the agents' read
// with the reader's own sight), between the number the Overview last showed
// them (`/api/projects/{p}/overview/seen`) and the log's head, so it says
// the same whether or not their chief of staff is awake. Beside the counts,
// the first sentence of the chief of staff's latest message in the
// reader's staff room (#1275), when it has written one; a click opens its
// tab.
//
// Three counts, each a thing the reader may act on:
// - merged: the project's PRs merged (`github.pr.merged`), each once;
// - went red: a branch whose check failed (`github.check.failed`), by the
//   branch it ran on (`heads`), when that's main or a card's session's
//   branch (the hub's sessions by card); a PR's base alone isn't the
//   branch, so a red PR into main isn't main going red;
// - wait on your review: PRs that became ready (`github.pr.ready`: green
//   and mergeable) where the reader is a requested reviewer now. An event
//   names no reviewers, so that's the reader's inbox (`review_requested`).
//
// The number moves when the reader leaves the Overview or hides its window:
// the head this page showed them is kept, and coming back counts from
// there. A hub without these routes (404) shows no line.

import { useEffect, useRef, useState } from "preact/hooks";
import type { EventsPage, InboxEntry, LogEvent, OverviewSeen } from "../shared/proto";
import { chat } from "../shared/chat-store";
import { platformFetch } from "../shared/maintenance";
import type { SessionsByCard } from "../shared/project-sessions";
import { useSubscribe } from "../shared/ui/hooks";

/** The types the line counts. */
export const BRIEFING_TYPES = ["github.pr.merged", "github.check.failed", "github.pr.ready"] as const;

/** Pages of the log read at most (200 each); past that the counts say "+". */
const PAGES_MAX = 5;

/** The branch a red check on counts as the project's own going red. */
const MAIN = "main";

export interface BriefingCounts {
  merged: number;
  red: number;
  /** null: the reader's inbox couldn't be read, so this isn't counted. */
  review: number | null;
  /** More happened than was read. */
  more: boolean;
}

const str = (v: unknown): string | undefined => (typeof v === "string" && v ? v : undefined);
const strs = (v: unknown): string[] => (Array.isArray(v) ? v.filter((x): x is string => typeof x === "string") : []);

/** The line's counts from `events`: `branches` are the cards' sessions'
 * branches, `reviews` the reader's review requests as `owner/name#N`. */
export function countBriefing(events: LogEvent[], branches: Set<string>, reviews: Set<string> | null, more = false): BriefingCounts {
  const merged = new Set<string>();
  const red = new Set<string>();
  const review = new Set<string>();
  for (const e of events) {
    const d = (e.data ?? {}) as Record<string, unknown>;
    if (e.type === "github.pr.merged") merged.add(e.subject);
    else if (e.type === "github.check.failed") {
      for (const h of strs(d.heads)) if (h === MAIN || branches.has(h)) red.add(`${str(d.repo) ?? ""}@${h}`);
    } else if (e.type === "github.pr.ready" && reviews) {
      const pr = `${str(d.repo) ?? ""}#${typeof d.pr === "number" ? d.pr : ""}`;
      if (reviews.has(pr)) review.add(pr);
    }
  }
  return { merged: merged.size, red: red.size, review: reviews ? review.size : null, more };
}

/** The counts in words: "4 merged · 1 went red · 2 wait on your review",
 * or none of them. */
export function briefingWords(c: BriefingCounts): string {
  const plus = c.more ? "+" : "";
  const parts = [c.merged && `${c.merged}${plus} merged`, c.red && `${c.red}${plus} went red`, c.review && `${c.review}${plus} ${c.review === 1 ? "waits" : "wait"} on your review`].filter(Boolean);
  return parts.length ? parts.join(" · ") : "nothing merged or red";
}

/** "18:04 today", "18:04 yesterday", or "18:04 Mon 16 Nov". */
export function sinceWords(at: number, now = Date.now()): string {
  const d = new Date(at);
  const t = d.toLocaleTimeString([], { hour: "numeric", minute: "2-digit" });
  const today = new Date(now);
  const yesterday = new Date(today.getFullYear(), today.getMonth(), today.getDate() - 1);
  if (d.toDateString() === today.toDateString()) return `${t} today`;
  if (d.toDateString() === yesterday.toDateString()) return `${t} yesterday`;
  return `${t} ${d.toLocaleDateString([], { weekday: "short", day: "numeric", month: "short" })}`;
}

/** A message's first sentence, short enough for the line. */
export function firstSentence(text: string, max = 140): string {
  const flat = text.replace(/\s+/g, " ").trim();
  const m = /^.+?[.!?](?=\s|$)/.exec(flat);
  const s = m ? m[0] : flat;
  return s.length > max ? `${s.slice(0, max - 1).trimEnd()}…` : s;
}

async function json<T>(path: string, init?: RequestInit): Promise<T | null> {
  const r = await platformFetch(path, { credentials: "same-origin", ...init });
  if (r.status === 404) return null;
  if (!r.ok) throw new Error(`HTTP ${r.status}`);
  return (await r.json()) as T;
}

const seenPath = (project: string) => `/api/projects/${encodeURIComponent(project)}/overview/seen`;

/** The reader's review requests on open PRs, as `owner/name#N`; null when
 * the inbox couldn't be read. */
async function reviewRequests(): Promise<Set<string> | null> {
  try {
    const r = await json<{ entries: InboxEntry[] }>("/api/inbox");
    return r && new Set(r.entries.filter((e) => e.kind === "pr" && e.state === "open" && e.reason === "review_requested").map((e) => `${e.repo}#${e.number}`));
  } catch {
    return null;
  }
}

/** What the line shows: the counts since `at`, and the head they run to. */
export interface Briefing {
  /** When the reader last looked; null the first time (nothing to count from). */
  at: number | null;
  head: number;
  counts: BriefingCounts | null;
}

/** The project's briefing, read when the Overview comes up and when the
 * reader comes back to its window; the head it showed is kept when they
 * leave or hide it. null until read, and where the hub has no such routes. */
export function useBriefing(project: string, byCard: SessionsByCard | null): Briefing | null {
  const [b, setB] = useState<Briefing | null>(null);
  const shown = useRef<number | null>(null);
  const branches = new Set((byCard?.groups ?? []).filter((g) => g.card).flatMap((g) => g.sessions.flatMap((s) => [s.branch ?? "", ...(s.kind === "machine" ? s.panes.map((p) => p.branch ?? "") : [])]).filter(Boolean)));
  const [events, setEvents] = useState<{ list: LogEvent[]; more: boolean; reviews: Set<string> | null } | null>(null);

  useEffect(() => {
    let live = true;
    const read = async () => {
      try {
        const seen = await json<OverviewSeen>(seenPath(project));
        if (!seen) return live && setB(null);
        const list: LogEvent[] = [];
        let after = seen.seq;
        let head = seen.seq;
        let more = false;
        for (let i = 0; ; i++) {
          const q = new URLSearchParams({ after: String(after), types: BRIEFING_TYPES.join(","), limit: "200" });
          const page = await json<EventsPage>(`/api/projects/${encodeURIComponent(project)}/events?${q}`);
          if (!page) return live && setB(null);
          head = page.head;
          // The first look: nothing to count from, only where the log is.
          if (seen.seq === 0) break;
          list.push(...page.events);
          after = page.next_after;
          if (after >= page.head) break;
          if (i + 1 >= PAGES_MAX) {
            more = true;
            break;
          }
        }
        const reviews = seen.seq === 0 ? null : await reviewRequests();
        if (!live) return;
        shown.current = head;
        setEvents(seen.seq === 0 ? null : { list, more, reviews });
        setB({ at: seen.seq === 0 ? null : seen.at_ms, head, counts: null });
      } catch {
        // The line waits for the next read.
      }
    };
    // Leaving or hiding: keep the head this page showed.
    const keep = () => {
      const head = shown.current;
      if (head === null) return;
      void platformFetch(seenPath(project), { method: "PUT", credentials: "same-origin", keepalive: true, headers: { "Content-Type": "application/json" }, body: JSON.stringify({ seq: head }) }).catch(() => {});
    };
    const vis = () => (document.visibilityState === "hidden" ? keep() : void read());
    void read();
    document.addEventListener("visibilitychange", vis);
    window.addEventListener("pagehide", keep);
    return () => {
      live = false;
      keep();
      document.removeEventListener("visibilitychange", vis);
      window.removeEventListener("pagehide", keep);
    };
  }, [project]);

  if (!b) return null;
  return { ...b, counts: events && countBriefing(events.list, branches, events.reviews, events.more) };
}

/** The latest message an agent wrote in `room` (the chief of staff's, in
 * the reader's staff room): its first sentence. Loads the room once. */
function useLatestBriefing(room: string | undefined): string | null {
  useSubscribe((fn) => chat.subscribe(fn));
  useEffect(() => {
    if (room) void chat.load(room).catch(() => {});
  }, [room]);
  if (!room) return null;
  const list = chat.messages(room)?.list ?? [];
  for (let i = list.length - 1; i >= 0; i--) {
    const m = list[i];
    if (m.agent && !m.deleted && m.text.trim()) return firstSentence(m.text);
  }
  return null;
}

/** The line itself: "since 18:04 yesterday: 4 merged · 1 went red", the
 * chief of staff's latest sentence beside it, opening its tab. */
export function BriefingLine({ b, staffRoom, open }: { b: Briefing | null; staffRoom?: string; open?: () => void }) {
  const said = useLatestBriefing(staffRoom);
  if (!b || b.at === null || !b.counts) return null;
  const counts = `since ${sinceWords(b.at)}: ${briefingWords(b.counts)}`;
  const inner = (
    <>
      <span class="ov-briefing-counts" data-ov-briefing-counts>
        {counts}
      </span>
      {said && (
        <span class="ov-briefing-said" data-ov-briefing-said>
          {said}
        </span>
      )}
    </>
  );
  return open ? (
    <button type="button" class="ov-briefing" title="Open your chief of staff's tab" data-ov-briefing={b.head} onClick={open}>
      {inner}
    </button>
  ) : (
    <p class="ov-briefing" data-ov-briefing={b.head}>
      {inner}
    </p>
  );
}
