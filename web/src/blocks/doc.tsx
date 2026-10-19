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
import type { Client } from "../client";
import type { PaneId } from "../proto";
import { DocBlock, markdown, type DocState } from "./doc-render";
import { registerBlock, type BlockView } from "./view";

// The renderer is doc-render.tsx's; the share page imports it from there.
export { DocBlock, type DocState, type ImageLoader } from "./doc-render";

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

/** What the app adds to a Doc block (#1066): sharing it with a link is the
 * owner's, and lives in ui/doc-share.tsx, which sets this when the app
 * loads. The viewer behind a share link never loads it, so it stays out of
 * that page. */
export const docHooks: { state?: (client: Client, id: PaneId, s: DocState) => void } = {};

/** The caches of the Doc views that are open, for a spec to look at. */
const open = new Map<PaneId, Images>();
(globalThis as { __arugulaDocImages?: (id: PaneId) => Map<string, string> | undefined }).__arugulaDocImages = (id) => open.get(id)?.cache;

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
    render(<DocBlock id={id} s={state} fresh={fresh} load={images.load} cached={images.cache} />, host);
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
      docHooks.state?.(client, id, s);
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
