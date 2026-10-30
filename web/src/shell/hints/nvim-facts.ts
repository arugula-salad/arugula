// nvim's hints (#1735): chips on a pane running nvim at the moments they
// help, a key tip on each nvim start, and the "nvim in Arugula" help card.
// Their facts come from what the daemon says of the pane: its foreground
// command, and the editor it reads there (`PaneInfo.editor`, #1730: nvim's
// file and line, its pid and version, and `offer_plugin` when arugula.nvim
// could be added, #1733). All the owner's: a guest's page says none.
//
// - nvim-connect: nvim without the plugin, which Arugula could add.
// - nvim-open-here: an nvim the daemon reads, and the Editor preference
//   (#1731's, ../open-in-editor) isn't nvim.
// - nvim-diff: Claude Code proposes an edit while nvim is connected.
// - nvim-key-tip: one tip of ./nvim-tips on every new nvim process in a
//   pane (its pid), each tip once before any repeats, per account (the
//   place in the list is the pref `nvim.tips.next`).
// - nvim-help: the help card, a pane modal over the first nvim pane; the
//   pane menu ("nvim help") and Help in the account menu open it again.

import { paneIds, type Client } from "../client";
import type { PaneId, PaneInfo } from "../../shared/proto";
import { pref, setPref } from "../../shared/seen";
import { editorPref, runsNvim } from "../open-in-editor";
import type { Fact, Hint } from "./index";
import { NVIM_TIPS, nextTip } from "./nvim-tips";

/** Arugula's docs page for nvim. */
export const NVIM_DOCS = "https://docs.arugula.io/nvim/";

/** The shown tab's panes, with what the daemon says of each, where nvim's
 * hints may be said: the owner's page. */
function tabPanes(client: Client): { id: PaneId; info: PaneInfo }[] {
  if (client.state?.roles) return [];
  const tab = client.tabView();
  if (!tab) return [];
  return paneIds(tab).flatMap((id) => {
    const info = client.info(id);
    return info ? [{ id, info }] : [];
  });
}

/** This host's pane, not another machine's: the daemon's operations on it
 * are this page's to call. */
export const local = (client: Client, id: PaneId, info: PaneInfo) => info.host == null && !client.machine(id);

export function nvimWithoutPlugin(client: Client): Fact | null {
  const p = tabPanes(client).find(({ id, info }) => info.editor?.offer_plugin && local(client, id, info));
  return p ? { pane: p.id, args: { pane: p.id } } : null;
}

/** Opening files in an nvim takes its RPC socket, so only an nvim the
 * daemon reads (its editor): which also lets `nvim-connect`, said first,
 * come before it once the daemon has looked. */
export function nvimNotTheEditor(client: Client): Fact | null {
  if (editorPref() === "nvim") return null;
  const p = tabPanes(client).find(({ info }) => info.editor?.app === "nvim");
  return p ? { pane: p.id, args: { pane: p.id } } : null;
}

export function claudeEditsBesideNvim(client: Client): Fact | null {
  const all = tabPanes(client);
  if (!all.some(({ info }) => info.diff)) return null;
  const p = all.find(({ info }) => info.editor?.app === "nvim");
  return p ? { pane: p.id, args: { pane: p.id, section: "diff" } } : null;
}

export function nvimRuns(client: Client): Fact | null {
  const p = tabPanes(client).find(({ info }) => runsNvim(info));
  return p ? { pane: p.id, args: { pane: p.id } } : null;
}

// ---- the key tip

/** The pref with the account's place in the tip list: the next to show. */
const NEXT = "nvim.tips.next";
/** Which tip each fact took (its key: the pane and its nvim's pid), so a
 * tip hidden by a tab switch comes back the same. */
const took = new Map<string, number>();

export function nvimStarted(client: Client): Fact | null {
  for (const { id, info } of tabPanes(client)) {
    const ed = info.editor;
    if (ed?.app !== "nvim" || ed.pid == null) continue;
    const key = `${id}:${ed.pid}`;
    const at = took.get(key) ?? nextTip(Number(pref(NEXT) ?? 0), ed.version);
    const tip = NVIM_TIPS[at];
    return { pane: id, key, args: { pane: id, at, tip: tip.id, keys: tip.keys, text: tip.text, help: tip.help } };
  }
  return null;
}

/** Shown for `f`: the first time for its nvim, its tip is taken, and the
 * account's place moves past it. */
export function tipTaken(f: Fact) {
  if (f.key === undefined || took.has(f.key)) return;
  const at = Number(f.args.at);
  took.set(f.key, at);
  setPref(NEXT, (at + 1) % NVIM_TIPS.length);
}

/** `gv reselects your last visual selection`. */
export function tipCopy(f: Fact): string {
  const text = String(f.args.text);
  return `${f.args.keys} ${text.charAt(0).toLowerCase()}${text.slice(1)}`;
}

// ---- the help card

/** This pane's nvim lacks arugula.nvim and Arugula could add it: the fact
 * nvim-connect's chip says. */
const canAddPlugin = (client: Client, f: Fact) => {
  const info = f.pane === undefined ? undefined : client.info(f.pane);
  return !!info?.editor?.offer_plugin && local(client, f.pane!, info);
};

export const NVIM_HELP: Hint = {
  // nvim's help card, the first time nvim runs in a pane. Short: the full
  // text is NVIM_DOCS.
  id: "nvim-help",
  place: "pane-modal",
  fact: nvimRuns,
  copy: () => "nvim in Arugula",
  body: () => ["Arugula follows this nvim, opens files in it, and shows Claude Code's edits here as a vimdiff."],
  buttons: [
    { label: "Edit in nvim", command: "editor.use-nvim", when: () => editorPref() !== "nvim" },
    { label: "Add arugula.nvim", command: "nvim.install-plugin", when: canAddPlugin },
    { label: "Read the docs", command: "nvim.docs" },
    { label: "Close", close: true },
  ],
  views: { shown: "nvim-help-shown", acted: "nvim-help-acted", closed: "nvim-help-closed", optedOut: "nvim-help-opted-out" },
};
