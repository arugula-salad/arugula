// The page behind a share link (#1066): `<viewer>/<id>#k=<key>`. It fetches
// the Doc's encrypted parts from where it was served, opens them with
// WebCrypto and draws the snapshot with the block's own renderer. The key
// is in the fragment, which the browser never sends.
//
// Nothing here may reach the app: no client, no telemetry, no service
// worker, no socket. The page's CSP allows scripts and requests from its
// own origin only, and images as data: URLs. Its import graph is checked
// from the build (docs/testing.md, "Shared Docs").

import { render } from "preact";
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

function Page({ view }: { view: View }) {
  if (view.kind === "loading") return <p class="dim doc-notice">Loading…</p>;
  if (view.kind !== "doc") {
    const [title, more] = NOTICE[view.kind]!;
    return (
      <main class="doc-notice" data-doc-notice={view.kind}>
        <h1>{title}</h1>
        <p class="dim">{more}</p>
      </main>
    );
  }
  return <DocBlock id={0} s={view.state} fresh={new Map()} load={view.load} />;
}

// The Doc's own look (#253) has a light set and a dark one, which style.css
// picks by `data-scheme`, as in the app; here the reader's system decides.
const light = matchMedia("(prefers-color-scheme: light)");
const scheme = () => (document.documentElement.dataset.scheme = light.matches ? "light" : "dark");
scheme();
light.addEventListener("change", scheme);

const root = document.getElementById("root")!;
const draw = (view: View) => {
  if (view.kind === "doc") document.title = view.state.title || "Doc";
  render(<Page view={view} />, root);
};
draw({ kind: "loading" });
void load().then(draw);
