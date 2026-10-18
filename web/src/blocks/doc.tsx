// Doc blocks (#963): a document an agent builds beside its pane, an outline
// first and then one section at a time. One scrolling column: the title,
// then each section's heading and either its body (Markdown, as the forge
// blocks draw one: no HTML ever; its charts are drawn here and its drawings
// shown through <img>, #964) or a placeholder saying what it will hold.
// A section filled while you watch gets a short highlight; nothing scrolls
// you. The daemon pushes every change (patches that `client.ts` merges into
// the whole state), so there is nothing here to poll.
//
// A picture (#965) is a file the daemon keeps: the state lists its name and
// rev and never its bytes, and the page asks for one by name (`doc_asset`, a
// viewer's call) the first time a section shows that rev.

import { render } from "preact";
import { useEffect, useMemo, useState } from "preact/hooks";
import type { Client } from "../client";
import type { PaneId } from "../proto";
import { plain } from "../ui/embeds";
import { Markdown } from "../ui/markdown";
import { registerBlock, type BlockView } from "./view";

interface Section {
  id: string;
  heading: string;
  intent: string;
  /** `null` while pending. */
  body: string | null;
  /** Bumped when the body is written, a refill too. */
  rev: number;
}

/** A picture the Doc keeps; the bytes are never in the state. */
interface Asset {
  name: string;
  mime: string;
  size: number;
  /** Changes when the name is written again. */
  rev: number;
}

interface DocState {
  title: string;
  entries: Section[];
  assets?: Asset[];
  rev: number;
  error?: string;
}

/** The Doc as Markdown, as the daemon's `capture --text` has it: a chart is
 * a line saying what it is and a drawing is `[drawing]`. */
function markdown(s: DocState): string {
  const parts = [`# ${s.title}\n`];
  for (const e of s.entries) {
    const body = e.body !== null ? `${plain(e.body).trimEnd()}\n` : `_(coming${e.intent ? `: ${e.intent}` : ""})_\n`;
    parts.push(`\n## ${e.heading}\n\n${body}`);
  }
  return parts.join("");
}

/** The pictures one Doc view has fetched, by `name/rev`, each as the data:
 * URL it is shown from. The view owns it and `dispose` empties it. A data:
 * URL and not a blob: URL because control serves this client with
 * `img-src 'self' data: https:` (no blob:), so a blob URL shows nothing for
 * someone who comes in through control; it needs no revoking either. */
interface Images {
  readonly cache: Map<string, string>;
  load(name: string, rev: number): Promise<string | null>;
  close(): void;
}

function makeImages(client: Client, id: PaneId): Images {
  const cache = new Map<string, string>();
  const asking = new Map<string, Promise<string | null>>();
  let closed = false;
  const load = (name: string, rev: number) => {
    const key = `${name}/${rev}`;
    const have = cache.get(key);
    if (have) return Promise.resolve(have);
    let p = asking.get(key);
    if (!p) {
      p = client
        .request("POST", `/api/blocks/${id}/call/doc_asset`, { name })
        .then(async (res) => {
          if (!res.ok) return null;
          const { mime, data } = await res.json<{ mime: string; data: string }>();
          const url = `data:${mime};base64,${data}`;
          if (closed) return url;
          // The name's older revs are done with.
          for (const k of cache.keys()) if (k.startsWith(`${name}/`)) cache.delete(k);
          cache.set(key, url);
          return url;
        })
        .catch(() => null)
        .finally(() => asking.delete(key));
      asking.set(key, p);
    }
    return p;
  };
  return {
    cache,
    load,
    close: () => {
      closed = true;
      cache.clear();
      asking.clear();
    },
  };
}

/** The caches of the Doc views that are open, for a spec to look at. */
const open = new Map<PaneId, Images>();
(globalThis as { __arugulaDocImages?: (id: PaneId) => Map<string, string> | undefined }).__arugulaDocImages = (id) => open.get(id)?.cache;

/** One picture of the Doc. Shown only from an asset the state lists; until it
 * arrives, or if it can't, the alt text stands in. */
function DocImage({ images, name, alt, rev }: { images: Images; name: string; alt: string; rev: number | undefined }) {
  const key = `${name}/${rev}`;
  const [url, setUrl] = useState<string | null>(images.cache.get(key) ?? null);
  const [failed, setFailed] = useState(false);
  useEffect(() => {
    if (rev === undefined) return;
    setFailed(false);
    const have = images.cache.get(key);
    if (have) return setUrl(have);
    // The picture shown stays until the new rev is here.
    let live = true;
    void images.load(name, rev).then((u) => live && (u ? setUrl(u) : setFailed(true)));
    return () => {
      live = false;
    };
  }, [images, key]);
  if (rev === undefined || failed) return <span class="doc-image-missing" data-doc-image-missing>{alt || name}</span>;
  if (!url) return <span class="doc-image-wait" role="img" aria-label={alt || name}>{alt || name}</span>;
  return <img class="doc-image" src={url} alt={alt || name} loading="lazy" onError={() => setFailed(true)} />;
}

function DocBlock({ id, s, fresh, images }: { id: PaneId; s: DocState | null; fresh: Map<string, number>; images: Images }) {
  // Drawn again when a picture is added or replaced, and not otherwise.
  const kept = JSON.stringify(s?.assets ?? []);
  const image = useMemo(() => {
    const revs = new Map((JSON.parse(kept) as Asset[]).map((a) => [a.name, a.rev]));
    return (src: string, alt: string, key: number) => {
      const name = src.slice("assets/".length);
      return <DocImage key={`${key}:${name}`} images={images} name={name} alt={alt} rev={revs.get(name)} />;
    };
  }, [kept, images]);
  if (!s) return <div class="browser-card dim">Loading…</div>;
  if (s.error) {
    return (
      <div class="browser-card error" data-doc-block={id}>
        <p>Can't show this Doc</p>
        <p class="dim">{s.error}</p>
      </div>
    );
  }
  const done = s.entries.filter((e) => e.body !== null).length;
  return (
    <div class="doc" data-doc-block={id}>
      <div class="doc-page">
        <h1 class="doc-title">{s.title || "Untitled"}</h1>
        {s.entries.length === 0 && <p class="dim doc-wait">Waiting for the outline…</p>}
        {s.entries.length > 0 && (
          <p class="dim doc-progress" data-doc-progress>
            {done} of {s.entries.length} sections written
          </p>
        )}
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
      </div>
    </div>
  );
}

registerBlock("doc", (client: Client, id): BlockView => {
  const host = document.createElement("div");
  host.className = "block block-doc";
  let state: DocState | null = null;
  /** The rev each section had when it was filled while this page watched. */
  const fresh = new Map<string, number>();
  const images = makeImages(client, id);
  open.set(id, images);
  let note = false;
  const draw = () => {
    // The daemon's features arrive just after its layout. Without the flag a
    // saved Doc is still on the machine, but it isn't shown (#347).
    if (client.features !== null && !client.flag("docs")) {
      host.className = "block block-unknown";
      render(null, host);
      host.textContent = "This block is in Labs";
      note = true;
      return;
    }
    if (note) host.textContent = "";
    note = false;
    host.className = "block block-doc";
    render(<DocBlock id={id} s={state} fresh={fresh} images={images} />, host);
  };
  const off = client.subscribe(draw);
  draw();
  return {
    host,
    setVisible: (v) => (host.style.display = v ? "" : "none"),
    update: (next) => {
      const s = next as DocState;
      // The first state we get is the baseline: only what changes after it
      // is a fill we watched.
      if (state) {
        const had = new Map(state.entries.map((e) => [e.id, e.rev]));
        for (const e of s.entries) if (e.rev > (had.get(e.id) ?? 0)) fresh.set(e.id, e.rev);
      }
      state = s;
      draw();
    },
    title: () => state?.title || "Doc",
    text: () => (state ? markdown(state) : ""),
    focus: () => host.querySelector<HTMLElement>(".doc-page")?.focus(),
    dispose: () => {
      off();
      images.close();
      open.delete(id);
      render(null, host);
      host.remove();
    },
  };
});
