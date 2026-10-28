// An editor block's colors from the page's palette (#1048): VS Code's
// workbench colors and syntax colors, made from the same tokens as the
// panes around it. The page sends them to the block (its `theme` call);
// the daemon writes them into the block's workspace, and VS Code applies a
// change to it at once. The extension's own themes (`ext/theme.json`,
// Slate, `ext/theme-light.json`, Latte, and `ext/theme-mocha.json`, Mocha)
// are these for those palettes.

import { legible, type Palette } from "../shared/appearance";
import type { NvimHl, NvimTheme } from "../shared/proto";

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

/** `#rrggbb` from a palette color (`#rrggbbaa` loses its alpha); none for
 * anything else. */
function rgb(c: string | undefined): string | undefined {
  return c && /^#[0-9a-f]{6}/i.test(c) ? c.slice(0, 7).toLowerCase() : undefined;
}

/** `a` toward `b` by `t` (0 to 1). */
function mix(a: string, b: string, t: number): string {
  const ch = (h: string, i: number) => parseInt(h.slice(1 + 2 * i, 3 + 2 * i), 16);
  const out = [0, 1, 2].map((i) => Math.round(ch(a, i) + (ch(b, i) - ch(a, i)) * t));
  return `#${out.map((v) => v.toString(16).padStart(2, "0")).join("")}`;
}

/** An nvim editor block's colors (#1734): the same palette as highlight
 * groups, on the terminal's background so nvim fills its pane as the shell
 * did. The daemon sends them to the block's arugula.nvim, which applies
 * them as the colorscheme `arugula` (`editors/nvim/lua/arugula/theme.lua`). */
export function nvimTheme(p: Palette): NvimTheme {
  const u = legible(p);
  const t = p.term;
  const bg = rgb(t.background) ?? rgb(u.bg)!;
  const fg = rgb(t.foreground) ?? rgb(u.fg)!;
  const c = (k: keyof typeof u) => rgb(u[k])!;
  const sel = rgb(t.selectionBackground) ?? mix(bg, c("muted"), 0.6);
  const groups: Record<string, NvimHl> = {};
  const set = (names: string[], h: NvimHl) => names.forEach((n) => (groups[n] = h));
  set(["Normal", "NormalNC"], { fg, bg });
  set(["NormalFloat"], { fg, bg: c("surface") });
  set(["FloatBorder"], { fg: c("muted"), bg: c("surface") });
  set(["FloatTitle"], { fg: c("accent"), bg: c("surface"), bold: true });
  set(["Cursor", "lCursor", "TermCursor"], { fg: bg, bg: rgb(t.cursor) ?? c("accent") });
  set(["CursorLine", "CursorColumn"], { bg: c("raised") });
  set(["ColorColumn"], { bg: c("surface") });
  set(["LineNr", "LineNrAbove", "LineNrBelow"], { fg: c("muted") });
  set(["CursorLineNr"], { fg: c("fg"), bold: true });
  set(["SignColumn", "FoldColumn"], { fg: c("muted"), bg });
  set(["Folded"], { fg: c("subtle"), bg: c("surface") });
  set(["WinSeparator", "VertSplit"], { fg: c("raised") });
  set(["StatusLine", "WinBar"], { fg: c("subtle"), bg: c("letterbox") });
  set(["StatusLineNC", "WinBarNC"], { fg: c("dim"), bg: c("letterbox") });
  set(["TabLine"], { fg: c("dim"), bg: c("surface") });
  set(["TabLineFill"], { bg: c("letterbox") });
  set(["TabLineSel"], { fg: c("fg"), bg, bold: true });
  set(["Visual"], { bg: sel });
  set(["Search"], { fg: c("on-color"), bg: c("warn") });
  set(["IncSearch", "CurSearch"], { fg: c("on-color"), bg: c("accent") });
  set(["Substitute"], { fg: c("on-color"), bg: c("danger") });
  set(["MatchParen"], { fg: c("accent"), bold: true, underline: true });
  set(["Pmenu"], { fg, bg: c("surface") });
  set(["PmenuSel"], { fg: c("fg"), bg: c("raised"), bold: true });
  set(["PmenuSbar"], { bg: c("raised") });
  set(["PmenuThumb"], { bg: c("muted") });
  set(["NonText", "Whitespace", "EndOfBuffer", "SpecialKey"], { fg: c("raised-hover") });
  set(["Conceal"], { fg: c("dim") });
  set(["Directory"], { fg: c("info") });
  set(["Title"], { fg: c("accent"), bold: true });
  set(["ErrorMsg"], { fg: c("danger") });
  set(["WarningMsg"], { fg: c("warn") });
  set(["MoreMsg", "Question"], { fg: c("ok") });
  set(["ModeMsg", "MsgArea"], { fg: c("subtle") });
  set(["QuickFixLine"], { bg: c("raised"), bold: true });
  // Syntax, as VS Code's token colors above.
  set(["Comment"], { fg: c("dim"), italic: true });
  set(["Constant", "Number", "Boolean", "Float"], { fg: c("peach") });
  set(["String", "Character"], { fg: c("ok") });
  set(["Identifier"], { fg });
  set(["Function"], { fg: c("info") });
  set(["Statement", "Keyword", "Conditional", "Repeat", "Exception", "Label"], { fg: c("accent") });
  set(["Operator"], { fg: c("teal") });
  set(["PreProc", "Include", "Define", "Macro", "PreCondit"], { fg: c("danger") });
  set(["Type", "StorageClass", "Structure", "Typedef"], { fg: c("warn") });
  set(["Special", "SpecialChar"], { fg: c("pink") });
  set(["Delimiter"], { fg: c("subtle") });
  set(["Tag"], { fg: c("info") });
  set(["Underlined"], { underline: true });
  set(["Error"], { fg: c("danger") });
  set(["Todo"], { fg: c("warn"), bold: true });
  set(["Added", "diffAdded"], { fg: c("ok") });
  set(["Removed", "diffRemoved"], { fg: c("danger") });
  set(["Changed", "diffChanged"], { fg: c("warn") });
  set(["DiffAdd"], { bg: mix(bg, c("ok"), 0.2) });
  set(["DiffDelete"], { fg: c("danger"), bg: mix(bg, c("danger"), 0.2) });
  set(["DiffChange"], { bg: mix(bg, c("warn"), 0.12) });
  set(["DiffText"], { bg: mix(bg, c("warn"), 0.3), bold: true });
  for (const [name, k] of [["Error", "danger"], ["Warn", "warn"], ["Info", "info"], ["Hint", "teal"], ["Ok", "ok"]] as const) {
    set([`Diagnostic${name}`], { fg: c(k) });
    set([`DiagnosticUnderline${name}`], { sp: c(k), undercurl: true });
  }
  // Tree-sitter's captures that read better than the groups they link to.
  set(["@variable"], { fg });
  set(["@variable.parameter"], { fg: c("danger") });
  set(["@variable.member", "@property", "@module"], { fg: c("subtle") });
  set(["@tag.attribute"], { fg: c("info") });
  set(["@markup.heading"], { fg: c("accent"), bold: true });
  set(["@punctuation.delimiter", "@punctuation.bracket"], { link: "Delimiter" });
  const ansi = ["black", "red", "green", "yellow", "blue", "magenta", "cyan", "white"] as const;
  const names = [...ansi, ...ansi.map((n) => `bright${n[0].toUpperCase()}${n.slice(1)}`)] as (keyof typeof t)[];
  const terminal = names.map((k) => rgb(t[k] as string | undefined));
  return { light: p.light, groups, terminal: terminal.every((x) => x) ? (terminal as string[]) : [] };
}
