// Comments on a Doc (#1258), in the margin beside the paragraph each is about.
// A comment is a message in the Doc's thread (the room the platform pushes the
// Doc's changes on), its anchor written into its text (doc-comment-text.ts),
// so the thread and the agents reading it see the passage too. Tap or click a
// paragraph, or select words in it, and the margin opens a box there; Save
// posts it. Anyone who may read the Doc may comment: the thread is its
// project's. Replies in a comment's thread (an agent's answer) show under it.
//
// The Doc page on the platform and its embed (the app's web view) both draw
// it around the Doc. A page too narrow for a margin lists the comments under
// the Doc instead, each with its quote, and the box comes up from the bottom.

import type { ComponentChildren } from "preact";
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "preact/hooks";
import type { ChatMessage } from "../proto";
import { chat } from "../chat-store";
import { AuthorName } from "../chat/agent-name";
import { ChatMarkdown } from "../chat/chat-markdown";
import { useSubscribe } from "../ui/hooks";
import { Time } from "../ui/time";
import { commentOf, commentText, quoteOf, type Anchor } from "./doc-comment-text";
import "./doc-comments.css";

/** What a paragraph is, for a comment: the blocks a section's Markdown draws. */
const BLOCKS = "p, li, blockquote, pre, td, th, h3, h4, h5, h6, dd, dt";
/** What a click on isn't a comment's: the Doc's own controls and frames. */
const NOT_TEXT = "a, button, input, textarea, select, label, summary, iframe, svg, canvas, [role=tab], [data-doc-proto]";
/** Under this the page has no room for a margin. */
const NARROW = 720;
/** The least room between two comments in the margin. */
const GAP = 8;

interface Props {
  /** The Doc's id, which its comments' links name. */
  doc: string;
  /** The Doc's page, as a comment's link names it. */
  url: string;
  /** Its sections' headings, by id. */
  headings: Map<string, string>;
  /** Who a comment wakes: the agent that wrote the Doc, as `@machine/session`, or empty. */
  mention: string;
  /** What `mention` is, for the box's line ("geek/docs-agent sees it"). */
  reaches?: string;
  /** Comments can't be made (a reader the thread refuses). */
  readOnly?: boolean;
  children: ComponentChildren;
}

/** One comment and where it goes. */
interface Placed {
  m: ChatMessage;
  anchor: Anchor | null;
  text: string;
  /** The paragraph it's beside, once found in the Doc. */
  at: HTMLElement | null;
}

/** The Doc's thread, opened once, and its messages. */
function useThread(doc: string): { room: string | null; messages: ChatMessage[]; error: string | null } {
  useSubscribe((fn) => chat.subscribe(fn));
  const [room, setRoom] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    let live = true;
    setRoom(null);
    chat.openObject(doc).then(
      (r) => {
        if (!live) return;
        setRoom(r.id);
        void chat.load(r.id).catch((e: Error) => live && setError(e.message));
      },
      (e: Error) => live && setError(e.message),
    );
    return () => void (live = false);
  }, [doc]);
  // The stream says a message came; read what's new when the Doc changes too.
  useEffect(() => (room ? chat.onObject(doc, () => void chat.readNew(room)) : undefined), [room, doc]);
  return { room, messages: room ? (chat.messages(room)?.list ?? []) : [], error };
}

/** The words of `el`, on one line. */
const wordsOf = (el: Element) => (el.textContent ?? "").replace(/\s+/g, " ").trim();

/** The paragraph of section `section` that `quote` is from: the first whose
 * words hold its start, else the section's heading; null when the section
 * isn't drawn (another tab, or gone). */
function findAnchor(root: HTMLElement, a: Anchor): HTMLElement | null {
  const sec = root.querySelector<HTMLElement>(`[data-section="${CSS.escape(a.section)}"]`);
  if (!sec) return null;
  const body = sec.querySelector(".doc-body");
  if (!body) return sec;
  const want = a.quote.replace(/…$/, "").slice(0, 60);
  if (want) for (const el of body.querySelectorAll<HTMLElement>(BLOCKS)) if (wordsOf(el).includes(want)) return el;
  return sec.querySelector<HTMLElement>(".doc-heading") ?? sec;
}

export function DocComments({ doc, url, headings, mention, reaches, readOnly, children }: Props) {
  const { room, messages, error } = useThread(doc);
  const wrap = useRef<HTMLDivElement>(null);
  const reader = useRef<HTMLDivElement>(null);
  const margin = useRef<HTMLDivElement>(null);
  const [narrow, setNarrow] = useState(false);
  /** The box: where it is and what it's on. */
  const [open, setOpen] = useState<{ anchor: Anchor; at: HTMLElement } | null>(null);
  const [tops, setTops] = useState<Map<string, number>>(new Map());
  const [, redraw] = useState(0);

  const comments = useMemo(
    () => messages.filter((m) => !m.deleted && !m.system && m.parent === undefined).map((m) => ({ m, ...commentOf(doc, m.text) })),
    [messages, doc],
  );

  // Where each comment goes, found again whenever the Doc or the comments are drawn.
  const placed: Placed[] = comments.map((c) => ({ ...c, at: c.anchor && reader.current ? findAnchor(reader.current, c.anchor) : null }));

  // The paragraphs with comments are marked in the Doc, and the box's too.
  useLayoutEffect(() => {
    const root = reader.current;
    if (!root) return;
    root.querySelectorAll(".doc-commented-p, .doc-comment-at").forEach((el) => el.classList.remove("doc-commented-p", "doc-comment-at"));
    for (const p of placed) p.at?.classList.add("doc-commented-p");
    open?.at.classList.add("doc-comment-at");
  });

  // In the margin, each card at its paragraph's height, pushed down past the one above.
  const layout = () => {
    const w = wrap.current;
    const side = margin.current;
    if (!w) return;
    const isNarrow = w.clientWidth < NARROW;
    if (isNarrow !== narrow) setNarrow(isNarrow);
    if (isNarrow || !side) return;
    const base = side.getBoundingClientRect().top;
    const want: [string, number, HTMLElement][] = [];
    if (open) want.push(["box", open.at.getBoundingClientRect().top - base, side.querySelector<HTMLElement>("[data-doc-comment-box]")!]);
    for (const p of placed) {
      const card = side.querySelector<HTMLElement>(`[data-doc-comment="${p.m.seq}"]`);
      if (p.at && card) want.push([String(p.m.seq), p.at.getBoundingClientRect().top - base, card]);
    }
    // The box keeps its place; the cards make room for it.
    want.sort((a, b) => a[1] - b[1] || (a[0] === "box" ? -1 : b[0] === "box" ? 1 : 0));
    const next = new Map<string, number>();
    let floor = 0;
    for (const [k, top, el] of want) {
      const at = Math.max(top, floor);
      next.set(k, at);
      floor = at + (el?.offsetHeight ?? 0) + GAP;
    }
    const same = next.size === tops.size && [...next].every(([k, v]) => Math.abs((tops.get(k) ?? -1) - v) < 1);
    if (!same) setTops(next);
  };
  useLayoutEffect(layout);
  useEffect(() => {
    const w = wrap.current;
    if (!w || typeof ResizeObserver === "undefined") return;
    // The Doc grows as it's written, its prototypes load, a tab changes.
    const watch = new ResizeObserver(() => redraw((n) => n + 1));
    watch.observe(w);
    if (reader.current) watch.observe(reader.current);
    return () => watch.disconnect();
  }, []);

  // A tap or click on a paragraph opens the box there; words selected in it are its quote.
  const pick = (e: MouseEvent) => {
    if (readOnly || !room) return;
    const t = e.target as Element | null;
    if (!t || t.closest(NOT_TEXT)) return;
    const block = t.closest<HTMLElement>(BLOCKS);
    const sec = t.closest<HTMLElement>("[data-section]");
    if (!block || !sec || !block.closest(".doc-body") || !reader.current?.contains(block)) return;
    const sel = getSelection();
    const chosen = sel && !sel.isCollapsed && block.contains(sel.anchorNode) ? sel.toString() : "";
    // A drag that selected words elsewhere isn't a pick.
    if (sel && !sel.isCollapsed && !chosen) return;
    setOpen({ anchor: { section: sec.dataset.section!, quote: quoteOf(chosen || wordsOf(block)) }, at: block });
  };

  const unplaced = placed.filter((p) => !p.at);
  const cards = (list: Placed[], where: "margin" | "list") =>
    list.map((p) => <Comment key={p.m.seq} p={p} room={room!} top={where === "margin" ? tops.get(String(p.m.seq)) : undefined} quote={where === "list"} />);

  return (
    <div class={narrow ? `doc-commented doc-commented-narrow${open ? " doc-commented-writing" : ""}` : "doc-commented"} ref={wrap} data-doc-comments={room ?? ""} data-doc-comment-count={comments.length}>
      <div class="doc-commented-doc" ref={reader} onClick={pick}>
        {children}
      </div>
      {!narrow && (
        <aside class="doc-margin" ref={margin} aria-label="Comments" data-doc-margin>
          {open && <Box key={`${open.anchor.section}:${open.anchor.quote}`} top={tops.get("box")} {...{ doc, url, headings, mention, reaches, room: room!, anchor: open.anchor }} done={() => setOpen(null)} />}
          {cards(
            placed.filter((p) => p.at),
            "margin",
          )}
          {!comments.length && !open && <p class="doc-margin-hint dim">{readOnly ? "No comments." : `Tap a paragraph to comment.${reaches ? ` ${reaches} sees it.` : ""}`}</p>}
          {error && <p class="doc-margin-hint dim" data-doc-comments-error>Comments can't be read: {error}</p>}
        </aside>
      )}
      {(narrow ? placed.length > 0 : unplaced.length > 0) && (
        <section class="doc-comment-list" aria-label={narrow ? "Comments" : "Other comments"} data-doc-comment-list>
          <h2 class="doc-comment-list-head">{narrow ? `Comments (${placed.length})` : "Other comments"}</h2>
          {cards(narrow ? placed : unplaced, "list")}
        </section>
      )}
      {narrow && open && (
        <div class="doc-comment-sheet">
          <Box key={`${open.anchor.section}:${open.anchor.quote}`} {...{ doc, url, headings, mention, reaches, room: room!, anchor: open.anchor }} quote done={() => setOpen(null)} />
        </div>
      )}
    </div>
  );
}

/** One comment: who, when, what, and the replies in its thread. */
function Comment({ p, room, top, quote }: { p: Placed; room: string; top?: number; quote: boolean }) {
  const n = p.m.thread?.replies ?? 0;
  const last = p.m.thread?.last ?? 0;
  useEffect(() => {
    if (n) void chat.threads.load(room, p.m.seq);
  }, [room, p.m.seq, n, last]);
  const replies = n ? (chat.threads.get(room, p.m.seq)?.replies ?? []) : [];
  const jump = () => p.at?.scrollIntoView({ block: "center", behavior: "smooth" });
  return (
    <article class="doc-comment" style={top === undefined ? undefined : { top: `${top}px` }} data-doc-comment={p.m.seq} data-anchored={p.at ? "" : undefined}>
      <header class="doc-comment-head">
        <AuthorName m={p.m} tag="span" /> · <Time at={p.m.at} />
      </header>
      {quote && p.anchor?.quote && (
        <button type="button" class="doc-comment-quote" onClick={jump} disabled={!p.at}>
          {p.anchor.quote}
        </button>
      )}
      <div class="doc-comment-text">
        <ChatMarkdown text={p.text} />
      </div>
      {replies.map((r) => (
        <div key={r.seq} class="doc-comment-reply" data-doc-comment-reply={r.seq}>
          <header class="doc-comment-head">
            <AuthorName m={r} tag="span" /> · <Time at={r.at} />
          </header>
          <div class="doc-comment-text">
            <ChatMarkdown text={r.text} />
          </div>
        </div>
      ))}
    </article>
  );
}

/** The box a comment is written in, on its paragraph. */
function Box({
  doc,
  url,
  headings,
  mention,
  reaches,
  room,
  anchor,
  top,
  quote,
  done,
}: {
  doc: string;
  url: string;
  headings: Map<string, string>;
  mention: string;
  reaches?: string;
  room: string;
  anchor: Anchor;
  top?: number;
  quote?: boolean;
  done: () => void;
}) {
  const [said, setSaid] = useState("");
  const [busy, setBusy] = useState(false);
  const [why, setWhy] = useState<string | null>(null);
  const field = useRef<HTMLTextAreaElement>(null);
  useEffect(() => field.current?.focus({ preventScroll: true }), []);
  const save = async (e: Event) => {
    e.preventDefault();
    if (!said.trim() || busy) return;
    setBusy(true);
    setWhy(null);
    try {
      await chat.post(room, commentText(url, headings.get(anchor.section) ?? "", anchor, said, mention));
      done();
    } catch (err) {
      setWhy((err as Error).message);
      setBusy(false);
    }
  };
  return (
    <form class="doc-comment doc-comment-box" style={top === undefined ? undefined : { top: `${top}px` }} onSubmit={(e) => void save(e)} data-doc-comment-box={doc}>
      {quote && anchor.quote && <p class="doc-comment-quote">{anchor.quote}</p>}
      <textarea
        ref={field}
        class="field"
        rows={3}
        value={said}
        placeholder="Comment…"
        aria-label="Your comment"
        data-doc-comment-input
        onInput={(e) => setSaid(e.currentTarget.value)}
        onKeyDown={(e) => {
          if (e.key === "Escape") {
            e.stopPropagation();
            done();
          } else if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) void save(e);
        }}
      />
      {why && (
        <p class="doc-comment-why" role="alert" data-doc-comment-why>
          Couldn't save it: {why}
        </p>
      )}
      <div class="doc-comment-buttons">
        <button type="submit" class="primary small" disabled={busy || !said.trim()} data-doc-comment-save>
          Save
        </button>
        <button type="button" class="secondary small" onClick={done} data-doc-comment-cancel>
          Cancel
        </button>
        {reaches && <span class="dim doc-comment-reaches">{reaches} sees it</span>}
      </div>
    </form>
  );
}
