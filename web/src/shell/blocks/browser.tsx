// Browser blocks (M6a): a port on a machine (a dev server, on the block's
// own origin), or a web page in a frame; for sites that refuse to be framed,
// a card that opens them in a new tab.

import { render } from "preact";
import type { Client } from "../client";
import { OPS, opPath } from "../ops";
import type { ListeningPort, PaneId, Ports } from "../../shared/proto";
import { askText, tellSetup, type PromptChoice } from "../../shared/ui/menu";
import { BlockFrame, RoutePill } from "./block-frame";
import { BlockError, BlockLoading, OpenOutside } from "./block-states";
import { registerBlock, type BlockView } from "./view";

export interface BrowserState {
  /** Where the page is now. */
  url: string;
  /** What the frame was last told to load. */
  src: string;
  title: string | null;
  framable: boolean | null;
  error: string | null;
  loading: boolean;
  reloads: number;
  back: string[];
  /** For a port: which, its path, and the machine (null: the daemon's host). */
  port: number | null;
  path: string | null;
  machine: string | null;
  /** For a port: its origin's key on control's block domain (M50). */
  site_key?: string;
}

const ADVANCED = "https://docs.arugula.io/advanced/";

/** Ports and editors in blocks are off on this machine: say what turns them
 * on, rather than fail (#171). */
export function blocksOff(what: "port" | "editor") {
  const [title, thing, anchor] =
    what === "port"
      ? ["Dev servers in blocks are off here", "A dev server beside its terminal", "#browser-blocks-on-ports"]
      : ["VS Code in blocks is off here", "VS Code beside your terminals", "#editor-blocks"];
  void tellSetup(
    title,
    `${thing} is optional: each block gets a web origin of its own. ` +
      "Join this machine to Arugula control and they open in Control with nothing else set up. " +
      "Without control, start arugulad with --block-listen 127.0.0.1:7701 for a browser on this computer " +
      "(arugulad install -- --block-listen 127.0.0.1:7701); from your phone or other machines it also needs a domain of yours.",
    { label: "How to turn it on (advanced setup)", href: ADVANCED + anchor },
  );
}

/** `:5173/path` for a port, else the URL. */
function shown(s: BrowserState) {
  return s.port !== null ? `:${s.port}${s.path ?? "/"}` : s.url;
}

/** The ports listening where the port would open (#150), the pane's own
 * first; null when the daemon can't say (an older one, through control too). */
export async function panePorts(client: Client, where: { split?: PaneId; host?: number; local?: boolean }): Promise<ListeningPort[] | null> {
  if (where.split === undefined) return null;
  const q = where.host !== undefined ? `?host=${where.host}` : where.local ? "?local=true" : "";
  try {
    const res = await client.request(OPS["pane.ports"].method, opPath("pane.ports", { id: where.split }) + q);
    if (!res.ok) return null;
    return (await res.json<Ports>()).ports;
  } catch {
    return null;
  }
}

/** The same, as answers to tap. None when the daemon can't say: then it's
 * typed. */
async function listening(client: Client, where: { split?: PaneId; host?: number; local?: boolean }): Promise<PromptChoice[]> {
  return ((await panePorts(client, where)) ?? []).map((p) => ({
    value: String(p.port),
    label: `:${p.port}`,
    hint: [p.process, p.pane ? "this pane" : ""].filter(Boolean).join(" · ") || undefined,
    title: `port ${p.port}${p.process ? `, ${p.process}` : ""}${p.pane ? ", started from this pane" : ""}`,
    strong: p.pane,
  }));
}

/**
 * Ask for a port and open it beside `split`: on machine `host`, or (`local`)
 * on the daemon's host even in a VM tab.
 */
export async function openPort(client: Client, where: { split?: PaneId; host?: number; local?: boolean }) {
  if (!client.showsBlocks()) return blocksOff("port");
  // No example number: one that looks real gets typed as it is (#150).
  const v = await askText("Open a port", "", "port number, optionally /path", listening(client, where));
  const m = v?.trim().match(/^:?(\d{1,5})(\/.*)?$/);
  if (!v) return;
  if (!m) {
    client.toast(`not a port: ${v}`);
    return;
  }
  openPortAt(client, where, Number(m[1]), m[2] ?? "/");
}

/**
 * Open `port` beside `split`, asking nothing (a hint's Open beside), and
 * show it. A phone shows one pane, and a split drops the one it was
 * showing: unless the port becomes the pane it shows, it drew the tab's
 * split, the port in half the screen beside the terminal (Arugula #299).
 */
export function openPortAt(client: Client, where: { split?: PaneId; host?: number; local?: boolean }, port: number, path = "/") {
  if (!client.showsBlocks()) return blocksOff("port");
  void client
    .openBlock(
      {
        type: "browser",
        config: { port, path },
        split: where.split ?? null,
        host: where.host ?? null,
        local: !!where.local,
      },
      "couldn't open that port",
    )
    .then((block) => block !== null && client.showWhenKnown(block));
}

function BrowserBlock({ client, id, s }: { client: Client; id: PaneId; s: BrowserState | null }) {
  if (!s) return <BlockLoading what="the page" />;
  const call = (method: string, args: unknown = {}) => void client.api(`/api/blocks/${id}/call/${method}`, args);
  const port = s.port !== null;
  const where = (() => {
    if (port) return `:${s.port}`;
    try {
      return new URL(s.url).host;
    } catch {
      return s.url;
    }
  })();
  return (
    <div class="browser">
      <div class="browser-bar">
        <button title="Back" aria-label="Back" disabled={s.back.length === 0} onClick={() => call("back")}>
          ←
        </button>
        <button title="Reload" aria-label="Reload" onClick={() => call("reload")}>
          ↻
        </button>
        {port && s.machine && (
          <span class="host-tag" title={`Port ${s.port} on ${s.machine}, a throwaway VM`}>
            VM
          </span>
        )}
        <form
          class="browser-url"
          onSubmit={(e) => {
            e.preventDefault();
            const v = (e.currentTarget.elements.namedItem("url") as HTMLInputElement).value;
            call("navigate", { url: v });
          }}
        >
          <input name="url" value={shown(s)} spellcheck={false} autocomplete="off" />
        </form>
        <RoutePill id={id} />
        {/^https?:/.test(s.url) && <OpenOutside href={s.url} />}
      </div>
      {s.error ? (
        <BlockError title={port ? `Nothing is answering on ${where}` : `Couldn't load ${where}`} detail={s.error} retry={() => call("reload")}>
          {port && <p class="block-error-detail">It shows again when the server is back.</p>}
        </BlockError>
      ) : s.framable === false ? (
        // The host, not the page's title: a long one flooded the card
        // (UI audit #177).
        <div class="block-empty" data-browser-refused>
          <p>{where} doesn't allow being shown inside another page.</p>
          <a class="primary" href={s.url} target="_blank" rel="noopener noreferrer">
            Open in a new tab ↗
          </a>
        </div>
      ) : s.framable === null ? (
        <BlockLoading what={where} />
      ) : port ? (
        // Its own origin, never the app's: the port's block origin, on
        // control's block domain or the daemon's own block site.
        // `allow-same-origin` means that origin.
        <BlockFrame
          client={client}
          id={id}
          siteKey={s.site_key}
          src={s.src}
          reloads={s.reloads}
          class="browser-frame"
          title={s.title ?? shown(s)}
          sandbox="allow-scripts allow-forms allow-same-origin"
        />
      ) : (
        // A web page, on its own site.
        <iframe
          key={`${s.src}#${s.reloads}`}
          class="browser-frame"
          src={s.src}
          title={s.title ?? s.url}
          sandbox="allow-scripts allow-forms allow-same-origin allow-popups"
          referrerpolicy="no-referrer"
        />
      )}
    </div>
  );
}

registerBlock("browser", (client, id): BlockView => {
  const host = document.createElement("div");
  host.className = "block block-browser";
  let state: BrowserState | null = null;
  const draw = () => render(<BrowserBlock client={client} id={id} s={state} />, host);
  draw();
  return {
    host,
    setVisible: (v) => (host.style.display = v ? "" : "none"),
    update: (s) => {
      state = s as BrowserState;
      draw();
    },
    title: () => state?.title ?? (state ? shown(state) : "browser"),
    text: () => (state ? `${state.title ?? ""}\n${shown(state)}`.trim() : ""),
    focus: () => host.querySelector<HTMLElement>("iframe, input")?.focus(),
    dispose: () => {
      render(null, host);
      host.remove();
    },
  };
});
