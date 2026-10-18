// An editor block's colors from the page's palette (#1048): VS Code's
// workbench colors and syntax colors, made from the same tokens as the
// panes around it. The page sends them to the block (its `theme` call);
// the daemon writes them into the block's workspace, and VS Code applies a
// change to it at once. The extension's own themes (`ext/theme.json`,
// Slate, and `ext/theme-light.json`, Latte) are these for those palettes.

import { legible, type Palette } from "./appearance";

export interface TokenRule {
  scope: string[];
  settings: { foreground: string; fontStyle?: string };
}

/** What the `theme` call takes. */
export interface EditorTheme {
  light: boolean;
  colors: Record<string, string>;
  tokenColors: TokenRule[];
}

/** `#rrggbb` at an alpha (`"80"`); `#rrggbbaa` as it is. */
const alpha = (hex: string, a: string) => (hex.length === 7 ? hex + a : hex);

export function editorTheme(p: Palette): EditorTheme {
  const u = legible(p);
  const t = p.term;
  const sel = t.selectionBackground ?? alpha(u.muted, "80");
  const colors: Record<string, string> = {
    foreground: u.fg,
    focusBorder: u.accent,
    "selection.background": alpha(sel, "80"),
    "widget.shadow": alpha(u.letterbox, "80"),
    "editor.background": u.bg,
    "editor.foreground": u.fg,
    "editorCursor.foreground": t.cursor ?? u.accent,
    "editor.selectionBackground": sel,
    "editor.inactiveSelectionBackground": alpha(u.muted, "40"),
    "editor.lineHighlightBackground": u.raised,
    "editor.lineHighlightBorder": "#00000000",
    "editorLineNumber.foreground": u.muted,
    "editorLineNumber.activeForeground": u.fg,
    "editorIndentGuide.background1": u.raised,
    "editorWhitespace.foreground": u["raised-hover"],
    "editorWidget.background": u.surface,
    "editorGroupHeader.tabsBackground": u.surface,
    "editorGroup.border": u.raised,
    "tab.activeBackground": u.bg,
    "tab.inactiveBackground": u.surface,
    "tab.activeForeground": u.fg,
    "tab.inactiveForeground": u.dim,
    "tab.border": u.letterbox,
    "tab.activeBorderTop": u.accent,
    "sideBar.background": u.surface,
    "sideBar.foreground": u.subtle,
    "sideBarSectionHeader.background": u.surface,
    "activityBar.background": u.letterbox,
    "activityBar.foreground": u.accent,
    "activityBar.inactiveForeground": u.dim,
    "activityBarBadge.background": u.accent,
    "activityBarBadge.foreground": u["on-color"],
    "titleBar.activeBackground": u.letterbox,
    "titleBar.activeForeground": u.fg,
    "titleBar.inactiveBackground": u.letterbox,
    "statusBar.background": u.letterbox,
    "statusBar.foreground": u.subtle,
    "statusBar.noFolderBackground": u.letterbox,
    "panel.background": u.bg,
    "panel.border": u.raised,
    "list.activeSelectionBackground": u.raised,
    "list.activeSelectionForeground": u.fg,
    "list.inactiveSelectionBackground": u.raised,
    "list.hoverBackground": alpha(u.raised, "80"),
    "list.focusOutline": u.accent,
    "input.background": u.raised,
    "input.foreground": u.fg,
    "dropdown.background": u.raised,
    "button.background": u.accent,
    "button.foreground": u["on-color"],
    "badge.background": u.accent,
    "badge.foreground": u["on-color"],
    "terminal.background": t.background ?? u.bg,
    "terminal.foreground": t.foreground ?? u.fg,
    "terminalCursor.foreground": t.cursor ?? u.accent,
  };
  const ansi = ["Black", "Red", "Green", "Yellow", "Blue", "Magenta", "Cyan", "White"] as const;
  for (const name of ansi) {
    const k = name.toLowerCase() as "black";
    const b = `bright${name}` as "brightBlack";
    if (t[k]) colors[`terminal.ansi${name}`] = t[k];
    if (t[b]) colors[`terminal.ansiBright${name}`] = t[b];
  }
  const rule = (scope: string[], foreground: string, fontStyle?: string): TokenRule => ({
    scope,
    settings: fontStyle ? { foreground, fontStyle } : { foreground },
  });
  const tokenColors = [
    rule(["comment", "punctuation.definition.comment"], u.dim, "italic"),
    rule(["keyword", "storage.type", "storage.modifier", "keyword.control"], u.accent),
    rule(["keyword.operator", "punctuation.separator", "punctuation.terminator"], u.teal),
    rule(["string", "punctuation.definition.string"], u.ok),
    rule(["constant.character.escape", "constant.regexp"], u.pink),
    rule(["constant.numeric", "constant.language", "constant.other"], u.peach),
    rule(["entity.name.function", "support.function", "meta.function-call"], u.info),
    rule(["entity.name.type", "entity.name.class", "support.type", "support.class", "entity.other.inherited-class"], u.warn),
    rule(["variable.parameter"], u.danger),
    rule(["entity.name.tag", "meta.attribute", "entity.other.attribute-name"], u.info),
    rule(["entity.name.namespace", "entity.name.module"], u.subtle),
    rule(["variable.other.property", "support.variable.property"], u.subtle),
    rule(["meta.macro", "entity.name.function.macro", "support.macro"], u.danger),
    rule(["markup.heading"], u.accent, "bold"),
    rule(["markup.inserted"], u.ok),
    rule(["markup.deleted"], u.danger),
    rule(["invalid"], u.danger),
  ];
  return { light: p.light, colors, tokenColors };
}
