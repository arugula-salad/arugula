// Open in editor follows the Editor preference (#1731): VS Code (an editor
// block, as before), nvim, or Ask each time. It's the account's when
// signed in on control's page, else this browser's (shared/seen's prefs,
// as Tips are). With none chosen it's VS Code, unless nvim has been seen
// running in one of your panes: then the first Open in editor asks, and
// remembers the answer.
//
// nvim is the daemon's to find (`pane.nvim_open`): the nvim already
// running in the pane, else in its tab, else a new pane beside it running
// nvim at the file. Every Open in editor (the pane menu and so the
// palette, the swarm's pane menu, Claude Code's diff cards, the diff and
// file blocks) comes through `openInEditor`.

import { useEffect } from "preact/hooks";
import type { ApiResponse, Client } from "./client";
import type { Fleet } from "./fleet";
import { OPS, opPath } from "./ops";
import type { NvimOpenRequest, NvimOpened, PaneId, PaneInfo } from "../shared/proto";
import { pref, seenReady, setPref, subscribeSeen } from "../shared/seen";
import { openMenu, type MenuItem } from "../shared/ui/menu";

export type Editor = "vscode" | "nvim" | "ask";

/** Settings › Editor's choices, in order. */
export const EDITORS: readonly (readonly [Editor, string])[] = [
  ["vscode", "VS Code"],
  ["nvim", "nvim"],
  ["ask", "Ask each time"],
];

const KEY = "editor";
/** When nvim was first seen running in one of your panes. */
const NVIM_SEEN = "editor:nvim-seen";

/** The preference, if one was chosen (or remembered). */
export function editorPref(): Editor | null {
  const v = pref(KEY);
  return v === "vscode" || v === "nvim" || v === "ask" ? v : null;
}

/** What Settings shows as chosen. */
export const editorShown = (): Editor => editorPref() ?? "vscode";

export const setEditorPref = (e: Editor) => setPref(KEY, e);

/** Whether a pane runs nvim, by what its daemon says: the editor it found
 * there (#1730), or the foreground command. */
export function runsNvim(info: PaneInfo | undefined | null): boolean {
  if (!info) return false;
  if (info.editor?.app === "nvim") return true;
  return info.command?.trim().split(/\s+/)[0]?.split("/").pop() === "nvim";
}

/** Note nvim in one of `client`'s panes, once: from then on, Open in
 * editor asks before it assumes VS Code. Only the owner's panes count. */
export function noteNvim(client: Client) {
  if (!seenReady() || pref(NVIM_SEEN) != null || client.state?.roles) return;
  if (client.state?.panes.some((p) => runsNvim(p))) setPref(NVIM_SEEN, Date.now());
}

/** Watch `client` for nvim in a pane, until the returned function is called. */
export function watchNvim(client: Client): () => void {
  const again = () => noteNvim(client);
  again();
  const offs = [client.subscribe(again), subscribeSeen(again)];
  return () => offs.forEach((f) => f());
}

/** Watches the shown machine for nvim in a pane; once, in the workspace. */
export function NvimWatch({ client }: { client: Client }) {
  useEffect(() => watchNvim(client), [client]);
  return null;
}

/** Where Open in editor was asked, and what it does there. */
export interface EditorTarget {
  /** A request to the daemon the pane is on. */
  request: (method: string, path: string, body?: unknown) => Promise<ApiResponse>;
  /** The pane: nvim is looked for there, then in its tab. */
  pane: PaneId;
  /** The file (whole, or relative to the pane's directory) and its line;
   * none for the pane's directory. */
  file?: string | null;
  line?: number | null;
  /** VS Code: what Open in editor did before the preference. */
  vscode: () => unknown;
  /** Show a pane (the nvim's) on that daemon. */
  show: (pane: PaneId) => void;
  toast: (msg: string) => void;
}

/** Open in editor, as the preference says. */
export async function openInEditor(t: EditorTarget): Promise<void> {
  const choice = await choose();
  if (choice === "vscode") t.vscode();
  else if (choice === "nvim") await inNvim(t);
}

/** The editor to open: the preference, or the person's answer. */
async function choose(): Promise<"vscode" | "nvim" | null> {
  const p = editorPref();
  if (p === "vscode" || p === "nvim") return p;
  if (p === "ask") return ask(false);
  // Not chosen: VS Code, unless nvim has run in one of your panes.
  return pref(NVIM_SEEN) != null ? ask(true) : "vscode";
}

/** Where the pointer last went down: the ask menu opens there. */
let last: { x: number; y: number } | null = null;
if (typeof addEventListener === "function")
  addEventListener(
    "pointerdown",
    (e) => {
      last = { x: e.clientX, y: e.clientY };
    },
    true,
  );

/** A small menu, VS Code or nvim, with Remember this. It resolves only
 * when one is chosen. */
function ask(remember: boolean): Promise<"vscode" | "nvim"> {
  const at = last ?? { x: innerWidth / 2, y: innerHeight / 3 };
  return new Promise((done) => {
    const show = (keep: boolean) => {
      const pick = (e: "vscode" | "nvim") => () => {
        if (keep) setEditorPref(e);
        done(e);
      };
      const items: MenuItem[] = [
        { header: "Open in" },
        { label: "VS Code", run: pick("vscode") },
        { label: "nvim", run: pick("nvim") },
        "separator",
        { label: "Remember this", hint: "Settings › Editor changes it", checked: keep, run: () => show(!keep) },
      ];
      openMenu({ clientX: at.x, clientY: at.y, preventDefault: () => {} }, items);
    };
    show(remember);
  });
}

async function inNvim(t: EditorTarget) {
  const body: NvimOpenRequest = { file: t.file ?? null, line: t.line ?? null };
  try {
    const res = await t.request(OPS["pane.nvim_open"].method, opPath("pane.nvim_open", { id: t.pane }), body);
    const v = await res.json<Partial<NvimOpened> & { error?: string }>().catch(() => null);
    if (!res.ok || v?.pane === undefined) return t.toast(v?.error ?? `couldn't open it in nvim (${res.status})`);
    t.show(v.pane);
  } catch {
    t.toast("couldn't open it in nvim");
  }
}

/** A target on `client`'s daemon, shown in its layout. */
export function clientTarget(client: Client, pane: PaneId, vscode: () => unknown, file?: string | null, line?: number | null): EditorTarget {
  return {
    request: (m, p, b) => client.request(m, p, b),
    pane,
    file,
    line,
    vscode,
    show: (p) => client.setActive(p),
    toast: (msg) => client.toast(msg),
  };
}

/** A target on one of the fleet's hosts, from the swarm: `back` leaves the
 * swarm for the pane shown. */
export function fleetTarget(
  fleet: Fleet,
  host: string,
  pane: PaneId,
  back: () => void,
  vscode: () => unknown,
  file?: string | null,
  line?: number | null,
): EditorTarget {
  return {
    request: (m, p, b) => fleet.request(host, m, p, b),
    pane,
    file,
    line,
    vscode,
    show: (p) => {
      back();
      fleet.open(host, p);
    },
    toast: (msg) => fleet.clientOf(host)?.toast(msg),
  };
}

/** `file` whole: as it is, or under `cwd` (a pane's directory). */
export function wholePath(file: string, cwd: string | null | undefined): string {
  return file.startsWith("/") || !cwd ? file : `${cwd.replace(/\/+$/, "")}/${file}`;
}

/** Where to open the file a unified diff changes: its first added line,
 * else its first hunk's start. */
export function diffLine(text: string): number | null {
  let at: number | null = null;
  let first: number | null = null;
  for (const l of text.split("\n")) {
    const m = /^@@ -\d+(?:,\d+)? \+(\d+)/.exec(l);
    if (m) {
      at = Number(m[1]);
      first ??= at;
    } else if (at === null || l.startsWith("-") || l.startsWith("\\")) {
      continue;
    } else if (l.startsWith("+")) {
      return Math.max(1, at);
    } else {
      at++;
    }
  }
  return first === null ? null : Math.max(1, first);
}
