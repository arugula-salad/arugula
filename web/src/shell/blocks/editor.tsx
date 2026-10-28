// Editor blocks (M27): VS Code (code-server) on a folder on the block's
// machine, in a frame on the block's own origin, like a browser block on a
// port. The daemon starts the server; until it's up the block says why.

import { render } from "preact";
import { useState } from "preact/hooks";
import { onAppearance, palette } from "../../shared/appearance";
import type { Client } from "../client";
import { editorTheme } from "../editor-theme";
import type { OpenRequest, PaneId } from "../../shared/proto";
import { BlockFrame, RoutePill, throughControl } from "./block-frame";
import { BlockEmpty, BlockError, BlockLoading, OpenOutside } from "./block-states";
import { blocksOff } from "./browser";
import { registerBlock, type BlockView } from "./view";

type Server =
  | { is: "stopped" }
  | { is: "fetching"; got: number; of: number }
  | { is: "starting" }
  | { is: "running" }
  | { is: "failed"; error: string };

export interface EditorState {
  folder: string;
  name: string;
  /** The active file, relative to the folder when it's inside it. */
  file: string | null;
  line: number | null;
  col: number | null;
  /** The lines around the cursor, from line `top`. */
  top: number | null;
  lines: string[];
  dirty: number;
  /** What the frame loads (fixed for the block's life). */
  src: string;
  /** Its origin's key on control's block domain (M50). */
  site_key?: string;
  reloads: number;
  server: Server | null;
  machine: string | null;
  error: string | null;
}

/**
 * Open an editor on a pane's machine, in its directory (the daemon fills
 * both in from `from`), in a tab of its own; or a file there, at `line`.
 */
export function openEditor(client: Client, from: PaneId, path?: string, line?: number | null) {
  if (!client.showsBlocks()) return blocksOff("editor");
  const config = path ? (line ? { path, line } : { path }) : {};
  void client.api("/api/blocks", { type: "editor", config, from_pane: from } satisfies OpenRequest, "couldn't open an editor");
}

const MB = (n: number) => `${Math.round(n / 1e6)} MB`;

function EditorBlock({ client, id, s }: { client: Client; id: PaneId; s: EditorState | null }) {
  // Reloaded here only: the daemon's count is for its own reloads.
  const [again, setAgain] = useState(0);
  if (!s) return <BlockLoading what="VS Code" />;
  const server = s.server ?? { is: "stopped" };
  // UI audit #177: the browser's bar, so an editor has one too: where it
  // is, which way it went, Reload, and ↗ where it has an address of its
  // own (through control, it opens only in its frame).
  const own = !throughControl(client, s.site_key) && /^https?:\/\//.test(s.src) ? s.src : null;
  const bar = (
    <div class="browser-bar editor-bar">
      <button title="Reload" aria-label="Reload" disabled={!s.src || server.is !== "running"} onClick={() => setAgain(again + 1)}>
        ↻
      </button>
      {s.machine && (
        <span class="host-tag" title={`On ${s.machine}, a throwaway VM`}>
          VM
        </span>
      )}
      <span class="editor-where" title={s.folder}>
        <b>{s.name || s.folder}</b>
        {s.file && <span class="dim"> · {s.file}</span>}
      </span>
      <RoutePill id={id} />
      {own && <OpenOutside href={own} />}
    </div>
  );
  let body;
  if (s.error || server.is === "failed") {
    body = (
      <BlockError
        title={`VS Code couldn't open ${s.name || "here"}`}
        detail={s.error ?? (server.is === "failed" ? server.error : "")}
        retry={s.error ? undefined : () => void client.api(`/api/blocks/${id}/call/start`, {})}
      />
    );
  } else if (server.is === "fetching") {
    const pct = server.of ? Math.floor((100 * server.got) / server.of) : 0;
    body = (
      <BlockEmpty data-editor="fetching">
        <p>Downloading VS Code (code-server) for the first time…</p>
        <p>{server.of ? `${pct}% of ${MB(server.of)}` : MB(server.got)}</p>
      </BlockEmpty>
    );
  } else if (!s.src) {
    body = <BlockLoading what={s.name || "the folder"} />;
  } else {
    body = (
      <div class="editor">
        {/* Its own origin, never the app's; `allow-same-origin` means that one. */}
        <BlockFrame
          client={client}
          id={id}
          siteKey={s.site_key}
          src={s.src}
          reloads={s.reloads + again}
          class="editor-frame"
          title={`VS Code: ${s.file ?? s.name}`}
          sandbox="allow-scripts allow-forms allow-same-origin allow-popups allow-downloads"
          allow="clipboard-read; clipboard-write"
        />
        {server.is === "starting" && <div class="editor-note">Starting VS Code…</div>}
      </div>
    );
  }
  return (
    <>
      {bar}
      {body}
    </>
  );
}

registerBlock("editor", (client, id): BlockView => {
  const host = document.createElement("div");
  host.className = "block block-editor";
  let state: EditorState | null = null;
  const draw = () => render(<EditorBlock client={client} id={id} s={state} />, host);
  draw();
  // Its colors follow this page's palette (#1048): sent once it's open on
  // its folder, and again when the palette changes. Quietly: a viewer who
  // may not change the block just keeps what it has.
  let sent = "";
  const sendTheme = () => {
    if (!state?.src || state.machine) return;
    const theme = editorTheme(palette());
    const key = JSON.stringify(theme);
    if (key === sent) return;
    sent = key;
    void client.request("POST", `/api/blocks/${id}/call/theme`, theme).catch(() => (sent = ""));
  };
  const offAppearance = onAppearance(sendTheme);
  return {
    host,
    setVisible: (v) => (host.style.display = v ? "" : "none"),
    update: (s) => {
      state = s as EditorState;
      draw();
      sendTheme();
    },
    title: () => (state ? (state.file ? `${state.file.split("/").pop()} — ${state.name}` : state.name || "editor") : "editor"),
    text: () => (state ? `${state.file ?? state.folder}${state.line ? `:${state.line}` : ""}\n${state.lines.join("\n")}`.trim() : ""),
    focus: () => host.querySelector<HTMLElement>("iframe")?.focus(),
    dispose: () => {
      offAppearance();
      render(null, host);
      host.remove();
    },
  };
});
