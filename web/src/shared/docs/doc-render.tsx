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
import { useEffect, useMemo, useState } from "preact/hooks";
import type { PaneId } from "../proto";
import { plain } from "../ui/embeds";
import { Markdown } from "../ui/markdown";
import { BlockError, BlockLoading } from "../ui/block-states";

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
  /** When it was shared with a link, and last updated (#1066); the link
   * itself is the owner's to ask for (`doc_shares`). */
  shared?: { made: number; updated: number } | null;
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
 * here, so a redraw doesn't flash. `byline` is who made it and when, for
 * the header (#253), from a view that knows: the block's state and a
 * share's snapshot don't say, so only a project Doc's page passes one. */
export function DocBlock({
  id,
  s,
  fresh,
  load,
  cached,
  byline,
  foot,
  share,
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
  const done = s.entries.filter((e) => e.body !== null).length;
  return (
    <div class="doc" data-doc-block={id}>
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
          <p class="doc-meta">
            {byline && <>{byline} · </>}
            {s.entries.length === 0 ? (
              <span class="doc-wait">Waiting for the outline…</span>
            ) : (
              <span class="doc-progress" data-doc-progress>
                {done} of {s.entries.length} sections written
              </span>
            )}
          </p>
        </header>
        {s.entries.map((e) => (
          <section key={e.id} class="doc-section" data-section={e.id} data-pending={e.body === null ? "" : undefined}>
            <h2 class="doc-heading">{e.heading}</h2>
            {e.body !== null ? (
              // A refill is a new element, so its highlight plays again.
              <div key={e.rev} class={fresh.get(e.id) === e.rev ? "doc-body doc-fresh" : "doc-body"}>
                <Markdown text={e.body} refs={null} embeds image={image} />
              </div>
            ) : (
              <div class="doc-pending">
                <span class="doc-coming">Coming…</span>
                {e.intent && <span class="doc-intent"> {e.intent}</span>}
              </div>
            )}
          </section>
        ))}
        {foot}
      </div>
    </div>
  );
}

