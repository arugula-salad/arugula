// The platform's links from the shell, inside the app (Arugula #989): the
// platform is this page, so a link the shell draws to one of its pages (a
// board's row, a native issue's, a session's card) goes through the app's
// router in place, rather than a tab of its own or a reload, so Back and
// Esc climb. Only in the machines' places, where the shell draws; the
// platform's own pages follow their links themselves.

import { TOP } from "../platform/paths.ts";

/** The app's path an `href` names: on this origin, and a route of the
 * app's (`TOP`); null for anything else (another site, `/api/…`). */
export function platformPath(href: string, origin: string): string | null {
  let url: URL;
  try {
    url = new URL(href, origin);
  } catch {
    return null;
  }
  if (url.origin !== origin) return null;
  const first = url.pathname.split("/")[1] ?? "";
  if (!(TOP as readonly string[]).includes(first)) return null;
  return url.pathname + url.search + url.hash;
}

/** Follow such links inside `within` (a selector) with `go`; a modified
 * click (a new tab) and a download stay the browser's. Stops when the
 * answer is called. */
export function followPlatformLinks(go: (path: string) => void, within: string, doc: Document = document): () => void {
  const click = (e: MouseEvent) => {
    if (e.defaultPrevented || e.button !== 0 || e.metaKey || e.ctrlKey || e.shiftKey || e.altKey) return;
    const a = e.target instanceof Element ? e.target.closest<HTMLAnchorElement>("a[href]") : null;
    if (!a || a.hasAttribute("download") || !a.closest(within)) return;
    const path = platformPath(a.getAttribute("href")!, doc.location.origin);
    if (path === null) return;
    e.preventDefault();
    go(path);
  };
  doc.addEventListener("click", click);
  return () => doc.removeEventListener("click", click);
}
