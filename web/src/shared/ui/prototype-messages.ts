// What a Doc's prototype and its frame say to each other (#506). The runner
// page (`runner.html`) spells the same two names; `doc-grammar.spec.ts`
// checks that it does. Later bridges (#508, #509) add their messages here.

/** From the frame's runner to its parent, once its listener is in. */
export const READY = "arugula.ready";
/** From the parent to the runner, once: the prototype's text and the
 * Doc's colour scheme. The runner replaces itself with it. */
export const HTML = "arugula.html";

/** From the parent to the runner's prototype, when the Doc's colour scheme
 * changes after it was handed over: its `color-scheme` follows, and nothing
 * else about it is touched. The only message it takes after the page. */
export const SCHEME = "arugula.scheme";

export type Scheme = "light" | "dark";

export interface HtmlMessage {
  type: typeof HTML;
  html: string;
  colorScheme: Scheme;
}

export interface SchemeMessage {
  type: typeof SCHEME;
  colorScheme: Scheme;
}

/** Whether `e` is the frame `win` saying it is ready. The parent listens on
 * its own window, where any frame or window can post: only the one it made
 * counts, and what the prototype itself posts later isn't `READY` to it. */
export function readyFrom(e: { source: unknown; data: unknown }, win: unknown): boolean {
  return !!win && e.source === win && (e.data as { type?: unknown } | null)?.type === READY;
}

export const schemeMessage = (colorScheme: Scheme): SchemeMessage => ({ type: SCHEME, colorScheme });

/** The message that hands the frame its page. */
export const htmlMessage = (html: string, colorScheme: Scheme): HtmlMessage => ({ type: HTML, html, colorScheme });

/** Where the shared Doc viewer's prototypes run: `proto.<block domain>` for
 * a viewer on `docs.<block domain>` (control's `blocks::proto_url`, on the same
 * scheme and port), and nowhere for one on control's own origin (`/d/…`), which
 * has no block domain to put it on. */
export function protoOfViewer(loc: { protocol: string; host: string }): string | null {
  return loc.host.startsWith("docs.") ? `${loc.protocol}//proto.${loc.host.slice("docs.".length)}` : null;
}
