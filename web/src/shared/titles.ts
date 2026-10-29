// What people read for a pane: its title without a program's status marks.
// A module of its own, with no imports, so anything can use it.

/** A title as people read it: a program's status marks in front of it
 * (Claude Code's ✳ and spinner, a prompt's ‹ or ❯, braille spinners) mean
 * nothing outside its terminal, and change while it works. A port's
 * `:5173/` keeps its colon (UI audit #177), as its address bar shows it, and
 * a pull request's `#84 title` its hash (#317). */
export function cleanTitle(t: string): string {
  return t.replace(/^(?:(?!:\d)[^\p{L}\p{N}~/.@(["'_#-])+/u, "").trim();
}
