// The key tips the nvim-key-tip chip shows, one each time nvim starts in a
// pane (#1735). Each is a key or command, one line on what it does, and the
// `:help` tag the chip's button opens.
//
// Ids are stable: seen state and the per-account order are kept by id, so
// an id is never renamed or reused, even after its tip is removed.
//
// The tips are Arugula's own words. Their source is nvim's :help (Neovim's
// documentation, under the Vim license), which is where each `help` tag
// points. `scripts/check-nvim-tips` checks every tag, and the online page
// it names, against a real nvim.

export interface NvimTip {
  /** Stable kebab-case id, never reused. */
  id: string;
  /** The keys or command as typed. */
  keys: string;
  /** What it does, one short line. */
  text: string;
  /** The exact `:help` tag. */
  help: string;
  /** Where the tag is in nvim's online docs, under
   * https://neovim.io/doc/user/ (`visual/#gv`, as nvim's `helptags.json`
   * says): the chip's button opens it when Arugula can't reach the
   * pane's nvim. `scripts/check-nvim-tips` checks it names the help file
   * `:help <tag>` opens. */
  page: string;
  /** The nvim release that brought it, when it's newer than 0.10. */
  since?: "0.11";
}

export const NVIM_TIPS: readonly NvimTip[] = [
  // Motions and jumps
  { id: "match-pair", keys: "%", text: "Jumps to the bracket, paren or brace that matches the one under the cursor", help: "%", page: "motion/#%25" },
  { id: "jump-back", keys: "<C-o>", text: "Goes back to where you were before the last jump; <C-i> goes forward", help: "CTRL-O", page: "motion/#CTRL-O" },
  { id: "change-list", keys: "g;", text: "Steps back through the places you changed; g, steps forward", help: "g;", page: "motion/#g%3B" },
  { id: "last-change", keys: "`.", text: "Jumps to the exact spot of your last change", help: "`.", page: "motion/#%60." },
  { id: "last-insert", keys: "gi", text: "Puts you back in insert mode where you last left it", help: "gi", page: "insert/#gi" },
  { id: "repeat-find", keys: ";", text: "Repeats the last f, t, F or T; , repeats it the other way", help: ";", page: "motion/#%3B" },
  { id: "go-file", keys: "gf", text: "Opens the file whose name is under the cursor", help: "gf", page: "editing/#gf" },

  // Text objects
  { id: "change-inner-word", keys: "ciw", text: "Changes the whole word under the cursor, from anywhere inside it", help: "iw", page: "motion/#iw" },
  { id: "delete-around-parens", keys: "da(", text: "Deletes the parentheses and everything between them", help: "a(", page: "motion/#a(" },
  { id: "change-inner-quotes", keys: "ci\"", text: "Changes the text inside the quotes around the cursor", help: "iquote", page: "motion/#iquote" },
  { id: "delete-inner-tag", keys: "dit", text: "Deletes what's between an HTML or XML tag pair, keeping the tags", help: "it", page: "motion/#it" },
  { id: "change-next-match", keys: "cgn", text: "Changes the next search match; . then does the same to the one after", help: "gn", page: "visual/#gn" },

  // Editing
  { id: "join-raw", keys: "gJ", text: "Joins the next line on without adding or removing spaces", help: "gJ", page: "change/#gJ" },
  { id: "increment", keys: "<C-a>", text: "Adds one to the number under or after the cursor; <C-x> subtracts", help: "CTRL-A", page: "change/#CTRL-A" },
  { id: "uppercase-motion", keys: "gUiw", text: "Uppercases the word under the cursor; gu lowercases", help: "gU", page: "change/#gU" },
  { id: "paste-indent", keys: "]p", text: "Pastes below, matching the current line's indent", help: "]p", page: "change/#%5Dp" },
  { id: "insert-register", keys: "<C-r>\"", text: "In insert mode, pastes a register without leaving insert mode", help: "i_CTRL-R", page: "insert/#i_CTRL-R" },
  { id: "insert-one-command", keys: "<C-o>", text: "In insert mode, runs one Normal command and comes back", help: "i_CTRL-O", page: "insert/#i_CTRL-O" },
  { id: "undo-in-time", keys: "g-", text: "Steps back through every undo state in time, other branches too", help: "g-", page: "undo/#g-" },
  { id: "earlier", keys: ":earlier 10m", text: "Puts the buffer back the way it was ten minutes ago", help: ":earlier", page: "undo/#%3Aearlier" },

  // Visual mode
  { id: "reselect", keys: "gv", text: "Reselects your last visual selection", help: "gv", page: "visual/#gv" },
  { id: "visual-other-end", keys: "o", text: "In visual mode, moves the cursor to the other end of the selection", help: "v_o", page: "visual/#v_o" },
  { id: "block-insert", keys: "<C-v>jjI", text: "Inserts the same text at the start of every line in a block", help: "v_b_I", page: "visual/#v_b_I" },
  { id: "block-append", keys: "<C-v>j$A", text: "Appends the same text to the end of every selected line", help: "v_b_A", page: "visual/#v_b_A" },
  { id: "visual-increment", keys: "g<C-a>", text: "Turns a selected column of equal numbers into 1, 2, 3 and so on", help: "v_g_CTRL-A", page: "change/#v_g_CTRL-A" },
  { id: "normal-on-lines", keys: ":'<,'>norm A;", text: "Runs Normal keys on each selected line, here adding a ; at the end", help: ":normal", page: "various/#%3Anormal" },

  // Search and substitute
  { id: "search-word", keys: "*", text: "Searches forward for the whole word under the cursor", help: "star", page: "pattern/#star" },
  { id: "global-delete", keys: ":g/pat/d", text: "Deletes every line that matches pat", help: ":g", page: "repeat/#%3Ag" },
  { id: "inverse-global", keys: ":v/pat/d", text: "Deletes every line that doesn't match pat", help: ":v", page: "repeat/#%3Av" },
  { id: "very-magic", keys: "/\\v", text: "Starts a pattern where ( ) | + ? { work without backslashes", help: "/\\v", page: "pattern/#%2F%5Cv" },
  { id: "empty-pattern", keys: ":%s//new/g", text: "Leaves the pattern empty to reuse your last search", help: "last-pattern", page: "pattern/#last-pattern" },
  { id: "repeat-substitute", keys: "g&", text: "Runs your last :s again, on every line", help: "g&", page: "change/#g%26" },
  { id: "cmdline-window", keys: "q:", text: "Opens your command history in a window you can edit and rerun from", help: "q:", page: "cmdline/#q%3A" },

  // Registers and macros
  { id: "yank-register", keys: "\"0p", text: "Pastes what you last yanked, even after deleting since", help: "quote0", page: "change/#quote0" },
  { id: "black-hole", keys: "\"_d", text: "Deletes without touching any register, so your yank stays", help: "quote_", page: "change/#quote_" },
  { id: "system-clipboard", keys: "\"+y", text: "Yanks to the system clipboard", help: "quoteplus", page: "provider/#quoteplus" },
  { id: "repeat-macro", keys: "@@", text: "Runs the macro you ran last again", help: "@@", page: "repeat/#%40%40" },
  { id: "append-macro", keys: "qA", text: "Records more keys onto the end of the macro in register a", help: "q", page: "repeat/#q" },
  { id: "repeat-command", keys: "@:", text: "Repeats the last command-line command", help: "@:", page: "repeat/#%40%3A" },

  // Windows, buffers and tabs
  { id: "only-window", keys: "<C-w>o", text: "Closes every window but this one", help: "CTRL-W_o", page: "windows/#CTRL-W_o" },
  { id: "equal-windows", keys: "<C-w>=", text: "Makes all windows the same size", help: "CTRL-W_=", page: "windows/#CTRL-W_%3D" },
  { id: "window-to-tab", keys: "<C-w>T", text: "Moves this window into a tab of its own", help: "CTRL-W_T", page: "windows/#CTRL-W_T" },
  { id: "alternate-buffer", keys: "<C-^>", text: "Switches to the buffer you were in before this one", help: "CTRL-^", page: "editing/#CTRL-%5E" },
  { id: "buffer-by-name", keys: ":b part", text: "Switches to a buffer by any part of its name, with Tab to complete", help: ":b", page: "windows/#%3Ab" },

  // Quickfix
  { id: "vimgrep", keys: ":vimgrep /pat/ **", text: "Searches every file below here and fills the quickfix list", help: ":vimgrep", page: "quickfix/#%3Avimgrep" },
  { id: "cdo", keys: ":cdo s/old/new/ | update", text: "Runs a command at every quickfix entry, here a substitute and save", help: ":cdo", page: "quickfix/#%3Acdo" },
  { id: "quickfix-next", keys: "]q", text: "Goes to the next quickfix entry; [q goes back", help: "]q", page: "quickfix/#%5Dq", since: "0.11" },

  // Folds
  { id: "fold-toggle", keys: "za", text: "Opens or closes the fold under the cursor", help: "za", page: "fold/#za" },
  { id: "fold-all", keys: "zM", text: "Closes every fold in the window; zR opens them all", help: "zM", page: "fold/#zM" },

  // nvim's own
  { id: "checkhealth", keys: ":checkhealth", text: "Checks nvim and your plugins and reports what's missing or broken", help: ":checkhealth", page: "health/#%3Acheckhealth" },
  { id: "comment-line", keys: "gcc", text: "Comments or uncomments the line; gc takes a motion or selection", help: "gcc", page: "various/#gcc" },
  { id: "diagnostic-next", keys: "]d", text: "Jumps to the next diagnostic; [d goes back", help: "]d-default", page: "tagsrch/#%5Dd-default" },
  { id: "diagnostic-float", keys: "<C-w>d", text: "Shows the diagnostic under the cursor in a floating window", help: "CTRL-W_d-default", page: "tagsrch/#CTRL-W_d-default" },
  { id: "lsp-rename", keys: "grn", text: "Renames the symbol under the cursor everywhere, through the LSP", help: "grn", page: "lsp/#grn", since: "0.11" },
  { id: "lsp-references", keys: "grr", text: "Lists the references to the symbol under the cursor in quickfix", help: "grr", page: "lsp/#grr", since: "0.11" },
  { id: "lsp-code-action", keys: "gra", text: "Shows the language server's code actions for the cursor", help: "gra", page: "lsp/#gra", since: "0.11" },
  { id: "blank-line", keys: "]<Space>", text: "Adds an empty line below the cursor; [<Space> adds one above", help: "]<Space>", page: "insert/#%5D%3CSpace%3E", since: "0.11" },
  { id: "inspect", keys: ":Inspect", text: "Shows the highlight groups and syntax under the cursor", help: ":Inspect", page: "lua/#%3AInspect" },
];

/** Whether nvim `version` (`0.11.6`) is `since` (`0.11`) or newer. An
 * unknown version takes every tip. */
export function nvimAtLeast(version: string | null | undefined, since: string): boolean {
  const v = /^v?(\d+)\.(\d+)/.exec(version ?? "");
  if (!v) return true;
  const [maj, min] = since.split(".").map(Number);
  return Number(v[1]) > maj || (Number(v[1]) === maj && Number(v[2]) >= min);
}

/** The tip to show next, from index `from` (the account's place in the
 * list): the first there or after it, round again, that nvim `version`
 * has. So each tip shows once before any repeats. */
export function nextTip(from: number, version?: string | null, tips: readonly NvimTip[] = NVIM_TIPS): number {
  const n = tips.length;
  const start = Number.isInteger(from) && from >= 0 ? from % n : 0;
  for (let i = 0; i < n; i++) {
    const at = (start + i) % n;
    const since = tips[at].since;
    if (!since || nvimAtLeast(version, since)) return at;
  }
  return start;
}

/** Where a tip's help is in nvim's online docs: its page, else nvim's own
 * lookup by tag. */
export function tipDocs(tip: Pick<NvimTip, "help"> & { page?: string }): string {
  return tip.page ? `https://neovim.io/doc/user/${tip.page}` : `https://neovim.io/doc/user/helptag/?tag=${encodeURIComponent(tip.help)}`;
}
