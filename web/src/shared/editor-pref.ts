// The Editor preference (#1731): what Open in editor opens. Here, apart
// from opening one (shell/open-in-editor.ts), so the app's Settings can
// draw it too (Arugula #1108): it's one of your prefs (./seen.ts), the
// account's once you're signed in.

import { pref, setPref } from "./seen";

export type Editor = "vscode" | "nvim" | "ask";

/** Settings › Editor's choices, in order. */
export const EDITORS: readonly (readonly [Editor, string])[] = [
  ["vscode", "VS Code"],
  ["nvim", "nvim"],
  ["ask", "Ask each time"],
];

const KEY = "editor";

/** The preference, if one was chosen (or remembered). */
export function editorPref(): Editor | null {
  const v = pref(KEY);
  return v === "vscode" || v === "nvim" || v === "ask" ? v : null;
}

/** What Settings shows as chosen. */
export const editorShown = (): Editor => editorPref() ?? "vscode";

export const setEditorPref = (e: Editor) => setPref(KEY, e);
