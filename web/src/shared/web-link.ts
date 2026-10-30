// A link from an agent or a program, to open: web pages only (`http:` and
// `https:`), never another scheme a browser or the system would hand to a
// program. The daemon checks agents' links too.

/** `url` when it's an http(s) address, else undefined. */
export function webLink(url: string | undefined | null): string | undefined {
  if (!url) return undefined;
  try {
    const u = new URL(url);
    return u.protocol === "http:" || u.protocol === "https:" ? url : undefined;
  } catch {
    return undefined;
  }
}
