// How a Doc is laid out (Arugula #827, #829, #830), apart from drawing it so
// a spec can run these in plain node.

import { bodyLines, fences, plain } from "../ui/embeds";

export type Layout = "doc" | "tabs" | "prototype";
/** How a tab goes in the address; see `DocBlock`'s `link`. */
export type Link = "page" | "share";

/** The layout a state names: a missing one, or one a newer machine made that
 * this build doesn't know, is `doc`. */
export const docLayout = (name: string | null | undefined): Layout => (name === "tabs" || name === "prototype" ? name : "doc");

export interface ProtoOnly {
  line: string;
  html: string;
  alt: string;
  section: string;
}

/** A prototype Doc's one section as its view draws it: the line under the
 * title, and the prototype. Null where the Doc isn't that (an older or odd
 * Doc, a section still pending, no fence or more than one), and then it is
 * drawn as a plain Doc: never a blank view. */
export function protoOnly(s: { entries: { id: string; body: string | null }[] }): ProtoOnly | null {
  const [only] = s.entries;
  if (s.entries.length !== 1 || only!.body === null) return null;
  const found = fences(only!.body);
  if (found.length !== 1 || found[0]!.lang !== "prototype") return null;
  const f = found[0]!;
  const lines = bodyLines(only!.body);
  const line = [...lines.slice(0, f.start), ...lines.slice(f.end)].join(" ").replace(/\s+/g, " ").trim();
  return { line: plain(line), html: f.code, alt: f.alt, section: only!.id };
}
