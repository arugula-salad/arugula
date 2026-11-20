// A comment on a Doc (#1258) is a message in the Doc's thread whose text says
// where it is: a first line linking to the Doc at its section (`…#s2`), the
// passage quoted, then what was said. Written out in the text, so the thread,
// chat and an agent reading it all see what it's about, and an agent knows
// which Doc and section to change. Parsed back here to put it in the margin
// beside its paragraph. One the reader edits into another shape is a plain
// message again, shown at the foot of the margin.

/** Where a comment is: a section of the Doc, and the words it's about. */
export interface Anchor {
  section: string;
  quote: string;
}

export interface DocComment {
  anchor: Anchor | null;
  /** What was said, without the anchor's lines. */
  text: string;
}

/** The longest quote a comment keeps, in characters. */
export const QUOTE_MAX = 280;

/** `quote` on one line, cut to `QUOTE_MAX` at a word where it can be. */
export function quoteOf(text: string): string {
  const one = text.replace(/\s+/g, " ").trim();
  if ([...one].length <= QUOTE_MAX) return one;
  const cut = [...one].slice(0, QUOTE_MAX).join("");
  const space = cut.lastIndexOf(" ");
  return `${(space > QUOTE_MAX / 2 ? cut.slice(0, space) : cut).trimEnd()}…`;
}

/** A heading in a link's text: no brackets to end it early. */
const linkText = (s: string) => s.replace(/[[\]\r\n]/g, " ").trim();

/** The message for a comment: `url` is the Doc's page, `heading` its
 * section's; `mention` (an `@machine/session`, or empty) goes on a line of
 * its own at the end. */
export function commentText(url: string, heading: string, a: Anchor, said: string, mention = ""): string {
  const head = `[On “${linkText(heading) || a.section}”](${url}#${a.section})`;
  const quote = a.quote ? `\n> ${quoteOf(a.quote)}` : "";
  return `${head}${quote}\n\n${said.trim()}${mention ? `\n\n${mention}` : ""}`;
}

const HEAD = /^\[On “[^\]\n]*”\]\((\S+)#([\w-]+)\)$/;

/** A message's text as a comment on `doc`: its anchor when its first line
 * links to `doc`'s page at a section, else none and the whole text. */
export function commentOf(doc: string, text: string): DocComment {
  const lines = text.split("\n");
  const m = HEAD.exec(lines[0] ?? "");
  // The Doc's id, with or without the slug a page's address puts after it.
  const page = m?.[1]!.split("/docs/")[1];
  if (!m || (page !== doc && !page?.startsWith(`${doc}-`))) return { anchor: null, text };
  let i = 1;
  const quoted: string[] = [];
  while (i < lines.length && lines[i]!.startsWith(">")) quoted.push(lines[i++]!.replace(/^> ?/, ""));
  return { anchor: { section: m[2]!, quote: quoted.join(" ").trim() }, text: lines.slice(i).join("\n").trim() };
}
