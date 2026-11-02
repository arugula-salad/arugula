// What a Doc's prototype may ask its parent for (#508): copy a value, and keep
// a few settings. The frame's origin is opaque, so it has neither a clipboard
// nor storage; `runner.html`'s shim posts requests (`prototype-messages.ts`)
// and this answers them. Only the frame the Doc made is heard, only the
// message shapes there, and nothing else is reachable.

import { createContext } from "preact";
import { useContext, useEffect, useRef, useState } from "preact/hooks";
import { COPY, COPY_MAX, KEY_MAX, SECTION_MAX, STORE_ALL, STORE_GET, STORE_SET, VALUE_MAX, reply, requestOf, type Request } from "./prototype-messages";

/** Which Doc and section a prototype is in: the store's key. The Doc is its
 * block id, a project Doc's id or a share link's id, so two Docs with a
 * section `s1` don't share sliders. */
export const ProtoScope = createContext<{ doc: string; section: string } | null>(null);

const storeKey = (s: { doc: string; section: string }) => `arugula.proto.${s.doc}.${s.section}`;

type Section = Record<string, unknown>;

function load(k: string): Section {
  const raw = localStorage.getItem(k);
  const v = raw ? JSON.parse(raw) : {};
  return v && typeof v === "object" && !Array.isArray(v) ? (v as Section) : {};
}

type Answer = { ok: boolean; value?: unknown; error?: string };

/** A store request against this section's object, or why not. Storage can
 * be missing or full (private windows, quota): every access is in the try. */
export function storeCall(scope: { doc: string; section: string } | null, r: Extract<Request, { type: typeof STORE_GET | typeof STORE_SET | typeof STORE_ALL }>): Answer {
  const none: Answer = { ok: false, error: "no storage" };
  if (!scope) return none;
  const k = storeKey(scope);
  try {
    const all = load(k);
    if (r.type === STORE_ALL) return { ok: true, value: all };
    if (r.key.length > KEY_MAX) return { ok: false, error: "key too long" };
    if (r.type === STORE_GET) return { ok: true, value: all[r.key] };
    const json = JSON.stringify(r.value);
    if (json === undefined) return { ok: false, error: "not JSON" };
    if (json.length > VALUE_MAX) return { ok: false, error: "value too big" };
    const next = { ...all, [r.key]: JSON.parse(json) };
    if (JSON.stringify(next).length > SECTION_MAX) return { ok: false, error: "store full" };
    localStorage.setItem(k, JSON.stringify(next));
    return { ok: true };
  } catch {
    return none;
  }
}

/** Write `text` to the clipboard: the async API, or a hidden textarea. */
async function writeClipboard(text: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    // Missing or refused: the old way.
  }
  const t = document.createElement("textarea");
  t.value = text;
  t.setAttribute("readonly", "");
  t.style.cssText = "position:fixed;top:0;left:0;opacity:0;pointer-events:none";
  document.body.append(t);
  t.select();
  try {
    return document.execCommand("copy");
  } catch {
    return false;
  } finally {
    t.remove();
  }
}

/** Answers `frame`'s bridge requests while it is `live`; `copied` is true for
 * a moment after a copy, for the frame's edge to say "Copied". */
export function useBridge(frame: { current: HTMLIFrameElement | null }, live: boolean): boolean {
  const scope = useContext(ProtoScope);
  const scopeNow = useRef(scope);
  scopeNow.current = scope;
  const [copied, setCopied] = useState(false);
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  useEffect(() => {
    if (!live) return;
    const on = (e: MessageEvent) => {
      const win = frame.current?.contentWindow;
      // The parent hears every window; only the frame it made counts.
      if (!win || e.source !== win) return;
      const r = requestOf(e.data);
      if (!r) return;
      const send = (a: Answer) => win.postMessage(reply(r.id, a.ok, a.value, a.error), "*");
      if (r.type !== COPY) return send(storeCall(scopeNow.current, r));
      if (r.text.length > COPY_MAX) return send({ ok: false, error: "text too long" });
      // A click in the frame activates the page above it too: no click, no copy.
      if (!navigator.userActivation?.isActive) return send({ ok: false, error: "needs a click" });
      void writeClipboard(r.text).then((ok) => {
        send(ok ? { ok: true } : { ok: false, error: "copy refused" });
        if (!ok) return;
        setCopied(true);
        clearTimeout(timer.current);
        timer.current = setTimeout(() => setCopied(false), 1500);
      });
    };
    addEventListener("message", on);
    return () => removeEventListener("message", on);
  }, [live, frame]);
  useEffect(() => () => clearTimeout(timer.current), []);
  return copied;
}
