// The page behind a share link (#1066): `<viewer>/<id>#k=<key>`. It fetches
// the Doc's encrypted parts from where it was served, opens them with
// WebCrypto and draws the snapshot with the block's own renderer. The key
// is in the fragment, which the browser never sends.
//
// Nothing here may reach the app: no client, no telemetry, no service
// worker, no socket. The page's CSP allows scripts, requests and fonts
// from its own origin only, and images as data: URLs. Its import graph is
// checked from the build (docs/testing.md, "Shared Docs").

import { render, type ComponentChildren } from "preact";
// The app's style.css comes in through doc-view.css's @import: imported
// here too, it becomes a chunk the app shares, linked after the app's own
// CSS, which turns the app's cascade around.
import "./doc-view.css";
import { DocBlock, type DocState } from "../shared/docs/doc-render";
import { Broken, keyFromHash, openSnapshot, toBase64, type Opened } from "./doc-link";

type View =
  | { kind: "loading" }
  | { kind: "gone" }
  | { kind: "broken" }
  | { kind: "offline" }
  | { kind: "doc"; state: DocState; load: (name: string, rev: number) => Promise<string | null> };

/** What control serves for the page's own path, `…/<id>`. */
function base(): { id: string; at: string } {
  const at = location.pathname.replace(/\/+$/, "");
  return { id: at.slice(at.lastIndexOf("/") + 1), at };
}

const IMAGES = new Set(["image/png", "image/jpeg", "image/gif", "image/webp"]);

function viewOf(o: Opened): View {
  const { manifest, blobs } = o;
  const urls = new Map<string, string>();
  manifest.assets.forEach((a, i) => {
    if (IMAGES.has(a.mime)) urls.set(a.name, `data:${a.mime};base64,${toBase64(blobs[i]!)}`);
  });
  const state: DocState = {
    title: manifest.title,
    entries: manifest.entries.map((e) => ({ ...e, rev: e.body === null ? 0 : 1 })),
    assets: manifest.assets.map((a) => ({ ...a, rev: 1 })),
    rev: 1,
  };
  return { kind: "doc", state, load: (name) => Promise.resolve(urls.get(name) ?? null) };
}

/** A part's bytes for `ver`, or null if control no longer serves that version. */
async function part(at: string, n: number, ver: number): Promise<Uint8Array | null> {
  const res = await fetch(`${at}/data/${n}?v=${ver}`);
  if (res.status === 404) return null;
  if (!res.ok) throw new Error(`${res.status}`);
  return new Uint8Array(await res.arrayBuffer());
}

async function load(): Promise<View> {
  const { id, at } = base();
  const key = keyFromHash(location.hash);
  if (!key || !/^[0-9a-f]{32}$/.test(id)) return { kind: "broken" };
  try {
    // An Update landing between the page's requests makes its parts a 404
    // for the version it began with: start again from `meta` once.
    for (let attempt = 0; attempt < 2; attempt++) {
      const res = await fetch(`${at}/data/meta`);
      if (res.status === 404) return { kind: "gone" };
      if (!res.ok) return { kind: "offline" };
      const meta = (await res.json()) as { ver?: unknown; parts?: unknown };
      const { ver, parts } = meta;
      if (!Number.isSafeInteger(ver) || !Number.isSafeInteger(parts) || (ver as number) < 1 || (parts as number) < 1 || (parts as number) > 48) {
        return { kind: "broken" };
      }
      const got: Uint8Array[] = [];
      let moved = false;
      for (let n = 0; n < (parts as number); n++) {
        const p = await part(at, n, ver as number);
        if (!p) {
          moved = true;
          break;
        }
        got.push(p);
      }
      if (moved) {
        if (attempt === 0) continue;
        return { kind: "gone" };
      }
      return viewOf(await openSnapshot(key, id, ver as number, got));
    }
    return { kind: "gone" };
  } catch (e) {
    return e instanceof Broken || e instanceof SyntaxError ? { kind: "broken" } : { kind: "offline" };
  }
}

const NOTICE: Record<string, [string, string]> = {
  gone: ["This Doc isn't shared any more", "Ask whoever sent you the link for a new one."],
  broken: ["This link is broken", "Copy it again from whoever sent it to you."],
  offline: ["Couldn't load this Doc", "Reload to try again."],
};

/** The site's leaf (its `#leaf` symbol), in the Doc's rule colour. */
function Leaf() {
  return (
    <svg class="doc-made-leaf" viewBox="0 0 40 40" aria-hidden="true">
      <path d="M20 38 L20 8" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" fill="none" />
      <ellipse cx="20" cy="8.5" rx="6.2" ry="7" fill="currentColor" />
      <ellipse cx="14" cy="18" rx="5" ry="3" transform="rotate(-28 14 18)" fill="currentColor" />
      <ellipse cx="26" cy="18" rx="5" ry="3" transform="rotate(28 26 18)" fill="currentColor" />
      <ellipse cx="15.2" cy="25.5" rx="3.8" ry="2.3" transform="rotate(-28 15.2 25.5)" fill="currentColor" />
      <ellipse cx="24.8" cy="25.5" rx="3.8" ry="2.3" transform="rotate(28 24.8 25.5)" fill="currentColor" />
      <ellipse cx="16.8" cy="31.5" rx="2.6" ry="1.6" transform="rotate(-28 16.8 31.5)" fill="currentColor" />
      <ellipse cx="23.2" cy="31.5" rx="2.6" ry="1.6" transform="rotate(28 23.2 31.5)" fill="currentColor" />
    </svg>
  );
}

/** Under a shared Doc and its notices, not in the block (#255). */
function Made() {
  return (
    <footer class="doc-made" data-doc-made>
      <a href="https://arugula.io" rel="noopener">
        <Leaf />
        Made with Arugula
      </a>
    </footer>
  );
}

/** A page with no Doc on it, on the Doc's own paper. */
function Paper({ children }: { children: ComponentChildren }) {
  return (
    <div class="doc doc-paper-only">
      <div class="doc-page">{children}</div>
    </div>
  );
}

function Page({ view }: { view: View }) {
  if (view.kind === "loading")
    return (
      <Paper>
        <p class="doc-loading">Loading…</p>
      </Paper>
    );
  if (view.kind !== "doc") {
    const [title, more] = NOTICE[view.kind]!;
    return (
      <Paper>
        <main class="doc-notice" data-doc-notice={view.kind}>
          <h1 class="doc-notice-title">{title}</h1>
          <p class="doc-notice-more">{more}</p>
        </main>
        <Made />
      </Paper>
    );
  }
  return <DocBlock id={0} s={view.state} fresh={new Map()} load={view.load} foot={<Made />} />;
}

// The Doc's own look (#253) has a light set and a dark one, which style.css
// picks by `data-scheme`, as in the app; here the reader's system decides.
const light = matchMedia("(prefers-color-scheme: light)");
const scheme = () => (document.documentElement.dataset.scheme = light.matches ? "light" : "dark");
scheme();
light.addEventListener("change", scheme);

const root = document.getElementById("root")!;
const draw = (view: View) => {
  // The Doc's title, or what the notice says; a preview bot reads the
  // page's own, "A shared Doc", which control can't do better than.
  document.title = view.kind === "doc" ? view.state.title || "Untitled" : view.kind === "loading" ? document.title : NOTICE[view.kind]![0];
  render(<Page view={view} />, root);
};
draw({ kind: "loading" });
void load().then(draw);
