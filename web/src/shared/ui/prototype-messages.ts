// What a Doc's prototype and its frame say to each other (#506). The runner
// page (`runner.html`) spells the same two names; `doc-grammar.spec.ts`
// checks that it does. Later bridges (#508, #509) add their messages here.
//
// Bridge (#508): the frame has no clipboard and no storage, so a prototype
// asks its parent through `window.arugula` (the shim `runner.html` writes
// before the page): `copy(text)`, and `store.get/set/all`, a per-viewer store
// the parent keeps in its own localStorage. Requests carry an id and the
// parent answers each with an `arugula.reply`. The parent answers only the
// frame it made, only these shapes, and `copy` only during a click.

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

/** Bridge requests, from the prototype to its parent (#508). */
export const COPY = "arugula.copy";
export const STORE_GET = "arugula.store.get";
export const STORE_SET = "arugula.store.set";
export const STORE_ALL = "arugula.store.all";
/** The parent's answer to one request, by its id. */
export const REPLY = "arugula.reply";

/** At most this much text to copy. */
export const COPY_MAX = 16 * 1024;
/** A store key's length, a value's serialised size and a section's total. */
export const KEY_MAX = 128;
export const VALUE_MAX = 8 * 1024;
export const SECTION_MAX = 64 * 1024;

export type Request =
  | { type: typeof COPY; id: number; text: string }
  | { type: typeof STORE_GET; id: number; key: string }
  | { type: typeof STORE_SET; id: number; key: string; value: unknown }
  | { type: typeof STORE_ALL; id: number };

export interface ReplyMessage {
  type: typeof REPLY;
  id: number;
  ok: boolean;
  value?: unknown;
  error?: string;
}

export const reply = (id: number, ok: boolean, value?: unknown, error?: string): ReplyMessage => {
  const r: ReplyMessage = { type: REPLY, id, ok };
  if (value !== undefined) r.value = value;
  if (error !== undefined) r.error = error;
  return r;
};

/** `data` as a bridge request, or null: any other shape is dropped. The
 * sizes of a value and of a section's total are the store's to check. */
export function requestOf(data: unknown): Request | null {
  const m = data as { type?: unknown; id?: unknown; text?: unknown; key?: unknown; value?: unknown } | null;
  if (!m || typeof m !== "object" || typeof m.id !== "number" || !Number.isSafeInteger(m.id)) return null;
  const id = m.id;
  switch (m.type) {
    case COPY:
      return typeof m.text === "string" ? { type: COPY, id, text: m.text } : null;
    case STORE_GET:
      return typeof m.key === "string" ? { type: STORE_GET, id, key: m.key } : null;
    case STORE_SET:
      return typeof m.key === "string" ? { type: STORE_SET, id, key: m.key, value: m.value } : null;
    case STORE_ALL:
      return { type: STORE_ALL, id };
    default:
      return null;
  }
}
