// The Doc, drawn (#963, #1066): the title, then each section's heading and
// either its body (Markdown, as the forge blocks draw one: no HTML ever; its
// charts are drawn here and its drawings shown through <img>, #964) or a
// placeholder saying what it will hold. The block (doc.tsx: a daemon's
// state, its pictures asked for by name) and the page behind a share link
// (doc-view.tsx: a snapshot's) both draw it, each with its own picture
// loader, and so does a project Doc's page on the platform (aru-118, which
// draws no pictures). Nothing here may reach the client: the share page
// imports this file and must stay free of it.

import type { ComponentChildren } from "preact";
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "preact/hooks";
import type { PaneId } from "../proto";
import { plain } from "../ui/embeds";
import { Markdown } from "../ui/markdown";
import { ProtoScope } from "../ui/prototype-bridge";
import { Prototype, ProtoFill } from "../ui/prototype-frame";
import { BlockError, BlockLoading } from "../ui/block-states";
import { DocTimes, madeAt } from "./doc-times";
import { docLayout, protoOnly, type Link, type ProtoOnly } from "./layout";

export interface Section {
  id: string;
  heading: string;
  intent: string;
  /** `null` while pending. */
  body: string | null;
  /** Bumped when the body is written, a refill too. */
  rev: number;
}

/** A picture the Doc keeps; the bytes are never in the state. */
export interface Asset {
  name: string;
  mime: string;
  size: number;
  /** Changes when the name is written again. */
  rev: number;
}

export interface DocState {
  title: string;
  entries: Section[];
  assets?: Asset[];
  rev: number;
  /** The Doc's id in the library (a block's number isn't stable); the
   * daemon's block has it, a snapshot doesn't (#508). */
  doc?: string;
  /** When it was shared with a link, and last updated (#1066); the link
   * itself is the owner's to ask for (`doc_shares`). */
  shared?: { made: number; updated: number } | null;
  /** A section was written after the link was last updated (#509). */
  shared_older?: boolean;
  /** The machine has put a copy of it on the platform (Arugula #708); the
   * page is the owner's to ask for (`doc_shares`). */
  platform?: boolean;
  /** How the Doc is laid out; absent, or a name this build doesn't know, is
   * `doc` (Arugula #827): read it with `docLayout`. */
  layout?: string | null;
  /** When it was made and last changed (ms), and by whom (Arugula #825): the
   * header says them. A daemon from before them, and a share from before
   * them, send none. */
  made?: number | null;
  updated?: number | null;
  by?: string;
  error?: string;
}

/** Where a picture comes from: its data: URL, or null if it can't. */
export type ImageLoader = (name: string, rev: number) => Promise<string | null>;

/** The Doc as Markdown, as the daemon's `capture --text` has it: a chart is
 * a line saying what it is and a drawing is `[drawing]`. */
export function markdown(s: DocState): string {
  const parts = [`# ${s.title}\n`];
  for (const e of s.entries) {
    const body = e.body !== null ? `${plain(e.body).trimEnd()}\n` : `_(coming${e.intent ? `: ${e.intent}` : ""})_\n`;
    parts.push(`\n## ${e.heading}\n\n${body}`);
  }
  return parts.join("");
}

/** The pictures a view has, by `name/rev`, each as the data: URL it is shown from. */
export type ImageCache = Map<string, string>;

/** One picture of the Doc. Shown only from an asset the state lists; until it
 * arrives, or if it can't, the alt text stands in. */
function DocImage({ load, cached, name, alt, rev }: { load: ImageLoader; cached?: ImageCache; name: string; alt: string; rev: number | undefined }) {
  const key = `${name}/${rev}`;
  const [url, setUrl] = useState<string | null>(cached?.get(key) ?? null);
  const [failed, setFailed] = useState(false);
  useEffect(() => {
    if (rev === undefined) return;
    setFailed(false);
    const have = cached?.get(key);
    if (have) return setUrl(have);
    // The picture shown stays until the new rev is here.
    let live = true;
    void load(name, rev).then((u) => live && (u ? setUrl(u) : setFailed(true)));
    return () => {
      live = false;
    };
  }, [load, key]);
  if (rev === undefined || failed) return <span class="doc-image-missing" data-doc-image-missing>{alt || name}</span>;
  if (!url) return <span class="doc-image-wait" role="img" aria-label={alt || name}>{alt || name}</span>;
  return <img class="doc-image" src={url} alt={alt || name} loading="lazy" onError={() => setFailed(true)} />;
}

/** The Doc, drawn. The block (a daemon's state, its pictures asked for by
 * name) and the viewer behind a share link (a snapshot's, #1066) both draw
 * it, each with its own `load`; `cached` is the block's pictures already
 * here, so a redraw doesn't flash. `byline` is who made it, for the
 * header (#253), from a view that knows: the block's state and a share's
 * snapshot don't say, so only a project Doc's page passes one. When it was
 * made and last changed is the state's (`made`, `updated`), under it. */
export function DocBlock({
  id,
  s,
  fresh,
  load,
  cached,
  byline,
  foot,
  share,
  docKey,
  link,
}: {
  id: PaneId;
  s: DocState | null;
  fresh: Map<string, number>;
  load: ImageLoader;
  cached?: ImageCache;
  byline?: ComponentChildren;
  /** Under the last section: the share viewer's "Made with Arugula" (#255). */
  foot?: ComponentChildren;
  /** At the header's right: the Copy link button (#411). */
  share?: ComponentChildren;
  /** What names this Doc to a prototype's kept settings (#508): the block's
   * id by default, a project Doc's id or a share link's id where `id` is 0. */
  docKey?: string;
  /** Where a tab is named in the address (Arugula #829): `page` is `#s2`, for
   * a page whose address is its own; `share` is `&s=s2` after a share link's
   * key. Left out, a tab is kept in the view only: the app's address is its
   * route (`#docs`, `#pane=…`). */
  link?: Link;
}) {
  // Drawn again when a picture is added or replaced, and not otherwise.
  const kept = JSON.stringify(s?.assets ?? []);
  const image = useMemo(() => {
    const revs = new Map((JSON.parse(kept) as Asset[]).map((a) => [a.name, a.rev]));
    return (src: string, alt: string, key: number) => {
      const name = src.slice("assets/".length);
      return <DocImage key={`${key}:${name}`} load={load} cached={cached} name={name} alt={alt} rev={revs.get(name)} />;
    };
  }, [kept, load, cached]);
  if (!s) return <BlockLoading what="the Doc" />;
  if (s.error) return <BlockError data-doc-block={id} title="Can't show this Doc" detail={s.error} />;
  const key = docKey ?? s.doc ?? String(id);
  const layout = docLayout(s.layout);
  const proto = layout === "prototype" ? protoOnly(s) : null;
  if (proto) return <PrototypeDoc id={id} s={s} proto={proto} share={share} docKey={key} foot={foot} />;
  const tabs = layout === "tabs" && s.entries.length > 0;
  const done = s.entries.filter((e) => e.body !== null).length;
  const times = madeAt(s) || s.updated ? <DocTimes doc={s} /> : null;
  const body = (e: Section) =>
    e.body !== null ? (
      // A refill is a new element, so its highlight plays again.
      <div key={e.rev} class={fresh.get(e.id) === e.rev ? "doc-body doc-fresh" : "doc-body"}>
        <ProtoScope.Provider value={{ doc: key, section: e.id }}>
          <Markdown text={e.body} refs={null} embeds image={image} />
        </ProtoScope.Provider>
      </div>
    ) : (
      <div class="doc-pending">
        <span class="doc-coming">Coming…</span>
        {e.intent && <span class="doc-intent"> {e.intent}</span>}
      </div>
    );
  return (
    <div class="doc" data-doc-block={id} data-layout={tabs ? "tabs" : undefined}>
      <div class="doc-page">
        <header class="doc-head">
          {share ? (
            <div class="doc-head-row">
              <h1 class="doc-title">{s.title || "Untitled"}</h1>
              {share}
            </div>
          ) : (
            <h1 class="doc-title">{s.title || "Untitled"}</h1>
          )}
          {(byline || times || s.entries.length === 0 || done < s.entries.length) && (
            <p class="doc-meta">
              {byline}
              {byline && times && " · "}
              {times}
              {(byline || times) && (s.entries.length === 0 || done < s.entries.length) && " · "}
              {s.entries.length === 0 ? (
                <span class="doc-wait">Waiting for the outline…</span>
              ) : (
                done < s.entries.length && (
                  <span class="doc-progress" data-doc-progress>
                    {done} of {s.entries.length} sections written
                  </span>
                )
              )}
            </p>
          )}
        </header>
        {tabs ? (
          <DocTabs entries={s.entries} fresh={fresh} docKey={key} link={link} body={body} />
        ) : (
          s.entries.map((e) => (
            <section key={e.id} class="doc-section" data-section={e.id} data-pending={e.body === null ? "" : undefined}>
              <h2 class="doc-heading">{e.heading}</h2>
              {body(e)}
            </section>
          ))
        )}
        {foot}
      </div>
    </div>
  );
}

/** What names a tab and its panel to each other: the Doc's key, with what an
 * id can't hold made safe, so two Docs on one page (`id` is 0 in the previews
 * and the share page) never share one. */
const tabIds = (key: string, section: string) => {
  const safe = `${key}-${section}`.replace(/[^\w-]/g, "_");
  return { tab: `doc-tab-${safe}`, panel: `doc-panel-${safe}` };
};

/** The tab a link names: `#s2`, or `&s=s2` after a share link's key. */
function tabInHash(link: Link | undefined): string | null {
  if (!link) return null;
  const m = link === "share" ? /^#k=[\w-]+&s=([\w-]+)$/.exec(location.hash) : /^#([\w-]+)$/.exec(location.hash);
  return m ? m[1]! : null;
}

/** Says the tab in the address, in place: a tab is not a page, so it never
 * adds a history entry. A share link's hash is its key, kept whole. */
function tabToHash(link: Link | undefined, section: string) {
  if (!link) return;
  const key = link === "share" ? /^#(k=[\w-]+)/.exec(location.hash) : null;
  if (link === "share" && !key) return;
  const hash = key ? `#${key[1]}&s=${section}` : `#${section}`;
  if (location.hash !== hash) history.replaceState(history.state, "", `${location.pathname}${location.search}${hash}`);
}

/** The tabs of a `tabs` Doc (Arugula #829): a strip of each section's heading,
 * sticky at the top of the Doc's scroll area and scrolling sideways when it is
 * narrow, and the chosen section's panel. Only that one is drawn, so a
 * prototype in another tab isn't running. Arrows, Home and End move between
 * tabs, and the tab chosen takes focus. */
function DocTabs({ entries, fresh, docKey, link, body }: { entries: Section[]; fresh: Map<string, number>; docKey: string; link?: Link; body: (e: Section) => ComponentChildren }) {
  const [chosen, choose] = useState<string | null>(() => tabInHash(link));
  const strip = useRef<HTMLDivElement>(null);
  // A tab that isn't there (the link's, or one the Doc lost) is the first.
  const at = Math.max(0, entries.findIndex((e) => e.id === chosen));
  const on = entries[at]!;
  // The tabs filled while the reader was on another, until they're opened.
  const seen = useRef(new Set<string>());
  seen.current.add(`${on.id}/${on.rev}`);
  const go = (i: number, focus = false) => {
    const e = entries[i]!;
    choose(e.id);
    tabToHash(link, e.id);
    if (focus) (strip.current?.children[i] as HTMLElement | undefined)?.focus();
  };
  // A link edited by hand, or followed from another to the same page.
  useEffect(() => {
    if (!link) return;
    const fn = () => choose(tabInHash(link));
    addEventListener("hashchange", fn);
    return () => removeEventListener("hashchange", fn);
  }, [link]);
  // The tab chosen stays in view in a strip that scrolls. By hand: scrollIntoView would scroll the Doc too.
  useLayoutEffect(() => {
    const bar = strip.current;
    const t = bar?.children[at] as HTMLElement | undefined;
    if (!bar || !t) return;
    if (t.offsetLeft < bar.scrollLeft) bar.scrollLeft = t.offsetLeft;
    else if (t.offsetLeft + t.offsetWidth > bar.scrollLeft + bar.clientWidth) bar.scrollLeft = t.offsetLeft + t.offsetWidth - bar.clientWidth;
  }, [on.id, entries.length]);
  const key = (e: KeyboardEvent) => {
    const n = entries.length;
    const to = e.key === "ArrowRight" ? (at + 1) % n : e.key === "ArrowLeft" ? (at + n - 1) % n : e.key === "Home" ? 0 : e.key === "End" ? n - 1 : -1;
    if (to < 0) return;
    e.preventDefault();
    go(to, true);
  };
  return (
    <>
      <div class="doc-tabs" role="tablist" aria-label="Sections" ref={strip} onKeyDown={key} data-doc-tabs>
        {entries.map((e, i) => {
          const ids = tabIds(docKey, e.id);
          const news = i !== at && e.body !== null && fresh.get(e.id) === e.rev && !seen.current.has(`${e.id}/${e.rev}`);
          return (
            <button
              key={e.id}
              type="button"
              role="tab"
              id={ids.tab}
              class="doc-tab"
              aria-selected={i === at}
              aria-controls={ids.panel}
              tabIndex={i === at ? 0 : -1}
              title={e.heading}
              data-doc-tab={e.id}
              data-pending={e.body === null ? "" : undefined}
              onClick={() => go(i)}
            >
              {e.heading}
              {news && <span class="doc-tab-new" data-doc-tab-new role="img" aria-label="written just now" />}
            </button>
          );
        })}
      </div>
      {entries.map((e, i) => {
        const ids = tabIds(docKey, e.id);
        return (
          <section key={e.id} class="doc-panel" role="tabpanel" id={ids.panel} aria-labelledby={ids.tab} hidden={i !== at} data-section={e.id} data-pending={e.body === null ? "" : undefined}>
            {i === at && body(e)}
          </section>
        );
      })}
    </>
  );
}

/** The least a prototype Doc's frame is, where the view around it has no
 * height of its own (a preview, a page that grows). */
const FILL_MIN = 360;

/** A prototype Doc (Arugula #830): its title, its one line, and the frame
 * filling the rest of the view, full bleed like an artifact. The frame is
 * sized from the view's own box, by an observer on it and not on the window,
 * because a Doc sits in a pane, a preview or a page. */
function PrototypeDoc({ id, s, proto, share, docKey, foot }: { id: PaneId; s: DocState; proto: ProtoOnly; share?: ComponentChildren; docKey: string; foot?: ComponentChildren }) {
  const root = useRef<HTMLDivElement>(null);
  const head = useRef<HTMLElement>(null);
  const fill = useRef<HTMLDivElement>(null);
  const [open, setOpen] = useState(false);
  useLayoutEffect(() => {
    const box = root.current;
    if (!box || !head.current || !fill.current) return;
    const size = () => {
      // A pane scrolls its Doc, so the Doc's box is the height to fill; on a
      // page that grows (overflow visible) it is what's left of the window.
      const room = getComputedStyle(box).overflowY === "visible" ? innerHeight - box.getBoundingClientRect().top - 16 : box.clientHeight;
      fill.current!.style.height = `${Math.max(FILL_MIN, Math.floor(room - head.current!.offsetHeight))}px`;
    };
    size();
    if (typeof ResizeObserver === "undefined") return;
    const watch = new ResizeObserver(size);
    watch.observe(box);
    watch.observe(head.current);
    return () => watch.disconnect();
  }, []);
  return (
    <div class="doc" data-doc-block={id} data-layout="prototype" ref={root}>
      <div class="doc-page doc-page-fill">
        <header class="doc-head doc-head-fill" ref={head}>
          <div class="doc-head-row">
            <h1 class="doc-title">{s.title || "Untitled"}</h1>
            {share}
          </div>
          {proto.line && (
            <button type="button" class={open ? "doc-line doc-line-open" : "doc-line"} aria-expanded={open} title={proto.line} data-doc-line onClick={() => setOpen(!open)}>
              {proto.line}
            </button>
          )}
        </header>
        <div class="md doc-fill" ref={fill} data-doc-fill>
          <ProtoFill.Provider value={true}>
            <ProtoScope.Provider value={{ doc: docKey, section: proto.section }}>
              <Prototype html={proto.html} alt={proto.alt} />
            </ProtoScope.Provider>
          </ProtoFill.Provider>
        </div>
        {foot}
      </div>
    </div>
  );
}
