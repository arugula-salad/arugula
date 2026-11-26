// A Doc's prototypes by file name (Arugula #1451): a prototype that goes to
// `views-motion.html` (a link, `location.assign`, `location.href =`) means
// the ```prototype section titled "Views motion" in its own Doc, and the
// frame shows that section instead. A file name is the fence's title,
// lowercased with dashes, and `.html`; the path before it and the query and
// hash after it don't count. Anything else a prototype goes to isn't a file
// of the Doc.

import { fences, prototypeInfo } from "./embeds";

/** A prototype another of the Doc's can go to. */
export interface ProtoFile {
  /** The fence's title, as the section says it. */
  title: string;
  html: string;
  /** The section it's in: its settings are its own. */
  section: string;
}

/** A title as a file's name: `Views motion` is `views-motion`. */
export const fileStem = (title: string) =>
  title
    .toLowerCase()
    .replace(/[^\p{L}\p{N}]+/gu, "-")
    .replace(/^-+|-+$/g, "");

/** Every titled prototype in a Doc's sections, in order. */
export function protoFiles(entries: { id: string; body: string | null }[]): ProtoFile[] {
  const out: ProtoFile[] = [];
  for (const e of entries) {
    if (e.body === null) continue;
    for (const f of fences(e.body)) {
      if (f.lang !== "prototype") continue;
      const { title } = prototypeInfo(f.alt);
      if (fileStem(title)) out.push({ title, html: f.code, section: e.id });
    }
  }
  return out;
}

/** The prototype `url` names, or null where it names none of them. The
 * first of two with the same name wins. */
export function protoTarget(url: string, files: ProtoFile[]): ProtoFile | null {
  let path: string;
  try {
    path = new URL(url, "http://proto.invalid/").pathname;
  } catch {
    return null;
  }
  let name = path.slice(path.lastIndexOf("/") + 1);
  try {
    name = decodeURIComponent(name);
  } catch {
    return null;
  }
  const stem = fileStem(name.replace(/\.html?$/i, ""));
  return stem ? (files.find((f) => fileStem(f.title) === stem) ?? null) : null;
}
