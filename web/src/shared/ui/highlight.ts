// Colours for a chat message's code blocks (#109), a few hundred bytes of
// scanner instead of a parser: comments, strings, numbers and a language's
// keywords, which is what makes a snippet in a message readable. (The
// editor's real parsers are CodeMirror's, in the shell's chunk; the platform
// page can't import those, and a message's block is rarely more than a
// screen.) Loaded when a block first draws, so a room without code never
// fetches it.

/** A piece of the code and its class ("" for none), `hl-*` in style.css's chat part. */
export type Piece = [string, string];

const C_LIKE = "if else for while do switch case break continue return new delete typeof instanceof in of try catch finally throw class extends super this static public private protected void int long char float double bool boolean true false null undefined nil const let var function async await import export from default as type interface enum struct impl fn pub mut use mod trait where match loop self Self let go func defer chan range package map select string def lambda";
const PY = "def class if elif else for while return import from as with try except finally raise pass break continue lambda yield in is not and or None True False self async await global nonlocal del assert";
const SH = "if then else elif fi for while do done case esac function in return export local echo cd exit set unset source alias";
const SQL = "select from where insert into values update set delete create table drop alter join left right inner outer on group by order limit having and or not null as distinct union primary key index";

const LANGS: Record<string, { kw: Set<string>; line: string; block: boolean; ci: boolean }> = {};
const add = (names: string[], kw: string, line: string, block: boolean, ci = false) => {
  const set = new Set(kw.split(" "));
  for (const n of names) LANGS[n] = { kw: set, line, block, ci };
};
add(["js", "javascript", "jsx", "ts", "typescript", "tsx", "json", "jsonc", "java", "kotlin", "c", "cpp", "c++", "cs", "csharp", "go", "golang", "rust", "rs", "swift", "php", "dart"], C_LIKE, "//", true);
add(["py", "python", "rb", "ruby", "yaml", "yml", "toml"], PY, "#", false);
add(["sh", "bash", "shell", "zsh", "console", "fish"], SH, "#", false);
add(["sql"], SQL, "--", true, true);

// Strings (with escapes), then numbers, then words, one pattern: the first
// that matches at a spot wins, so a "//" inside a string stays a string.
const strings = String.raw`"(?:\\.|[^"\\\n])*"?|'(?:\\.|[^'\\\n])*'?|` + "`(?:\\\\.|[^`\\\\])*`?";
const number = String.raw`\b(?:0x[\da-f]+|\d[\d_]*\.?\d*(?:e[+-]?\d+)?)\b`;

export function highlight(code: string, lang: string): Piece[] | null {
  const l = LANGS[lang.toLowerCase()];
  if (!l) return null;
  const esc = (s: string) => s.replace(/[.*+?^${}()|[\]\\/-]/g, "\\$&");
  const comment = `${esc(l.line)}[^\\n]*${l.block ? `|/\\*[\\s\\S]*?(?:\\*/|$)` : ""}`;
  const re = new RegExp(`(${comment})|(${strings})|(${number})|([A-Za-z_$][\\w$]*)`, "gi");
  const out: Piece[] = [];
  let at = 0;
  for (const m of code.matchAll(re)) {
    if (m.index! > at) out.push([code.slice(at, m.index), ""]);
    const cls = m[1] ? "hl-c" : m[2] ? "hl-s" : m[3] ? "hl-n" : l.kw.has(l.ci ? m[4].toLowerCase() : m[4]) ? "hl-k" : "";
    out.push([m[0], cls]);
    at = m.index! + m[0].length;
  }
  if (at < code.length) out.push([code.slice(at), ""]);
  return out;
}
