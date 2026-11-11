// A prototype in a Doc (#506): the page in a ```prototype section, run in a
// sandboxed frame. The frame is `sandbox="allow-scripts"` and nothing else, so
// its origin is opaque: no cookies or storage, no reaching the Doc's page, the
// machine or the account, and no navigating the window above it. Inside, a CSP
// (`runner.html`) lets it run its own inline scripts and styles and loads
// nothing, not even a fetch.
//
// A srcdoc frame inherits its parent's CSP, and every page that shows a Doc has
// one (`script-src 'self'`), under which the runner's inline scripts would never
// run: the frame loads the same page from control's proto origin, which serves
// it under a CSP of its own and which control's pages' `frame-src` names, or on a
// daemon's own page from the daemon (`/proto/`, under the same CSP). With no
// proto origin (no block domain) the section says so. By `srcdoc` is what's left
// for a page that says nothing.
//
// On the shared viewer, where the reader is a stranger, nothing is framed until
// they click "Run prototype". In the Doc block and on project Docs it runs at once.

import { useContext, useEffect, useLayoutEffect, useRef, useState } from "preact/hooks";
import { useScheme } from "./drawing";
import { checkPrototype, prototypeInfo } from "./embeds";
import { ProtoScope, useBridge } from "./prototype-bridge";
import { escapeFrom, htmlMessage, readyFrom, schemeMessage } from "./prototype-messages";
import runner from "./runner.html?raw";
import { CloseIcon, ExpandIcon } from "./icons";

/** The frame's whole sandbox: scripts, and nothing that would give it a way out. */
export const SANDBOX = "allow-scripts";

interface Mode {
  /** Where the runner loads from: `undefined`, by `srcdoc`; a proto origin
   * (`https://proto.<block domain>`) or a daemon's runner (`<its page>/proto`),
   * at `/` under it; `null`, nowhere (the page can't run one). */
  origin: string | null | undefined;
  /** Whether a reader has to click before anything is framed. */
  click: boolean;
}
let mode: Mode = { origin: undefined, click: false };
const listeners = new Set<() => void>();

/** Said once by the page that knows (the shared viewer, the platform page). */
export function setProtoOrigin(origin: string | null | undefined, click = mode.click) {
  if (origin === mode.origin && click === mode.click) return;
  mode = { origin, click };
  listeners.forEach((fn) => fn());
}

function useMode(): Mode {
  const [, bump] = useState(0);
  useEffect(() => {
    const fn = () => bump((n) => n + 1);
    listeners.add(fn);
    return () => void listeners.delete(fn);
  }, []);
  return mode;
}

/** Loads of the frame that are the runner's own: its first, and the one
 * `document.close()` fires when it has written the prototype. A third is the
 * prototype navigating itself. */
export const OWN_LOADS = 2;

/** Full window rides on the top layer (#507): the box itself is a manual
 * popover, so the same iframe is drawn over the window without moving in the
 * DOM, which would reload it and lose its state. `position: fixed` wouldn't do:
 * the pane area is CSS-scaled, and a transform makes `fixed` relative to it. */
const canFull = typeof HTMLElement !== "undefined" && "showPopover" in HTMLElement.prototype;

/** A reader's click to run, kept per Doc and section (`ProtoScope`) so a new
 * version of the section (#509) doesn't ask again. */
const armedKeys = new Set<string>();

export function Prototype({ html, alt }: { html: string; alt: string }) {
  const { origin, click } = useMode();
  const scheme = useScheme();
  const frame = useRef<HTMLIFrameElement>(null);
  const loads = useRef(0);
  const handed = useRef(false);
  const schemeNow = useRef(scheme);
  schemeNow.current = scheme;
  const scope = useContext(ProtoScope);
  const armKey = scope ? `${scope.doc}/${scope.section}` : null;
  const [armed, arm] = useState(() => armKey !== null && armedKeys.has(armKey));
  const setArmed = (on: boolean) => {
    if (on && armKey !== null) armedKeys.add(armKey);
    arm(on);
  };
  const [left, setLeft] = useState(false);
  const [round, setRound] = useState(0);
  const [full, setFull] = useState(false);
  const box = useRef<HTMLDivElement>(null);
  const { title, height } = prototypeInfo(alt);
  const why = checkPrototype(html);
  const live = !why && origin !== null && (!click || armed) && !left;
  const copied = useBridge(frame, live);
  // A layout effect: the listener is there before the frame's first task, in
  // which the runner says it is ready.
  useLayoutEffect(() => {
    if (!live) return;
    // The first time the frame says it is ready, and no other.
    handed.current = false;
    const on = (e: MessageEvent) => {
      if (handed.current || !readyFrom(e, frame.current?.contentWindow)) return;
      handed.current = true;
      // An opaque origin has no name to target: "*", for this one message.
      frame.current!.contentWindow!.postMessage(htmlMessage(html, schemeNow.current), "*");
    };
    addEventListener("message", on);
    return () => removeEventListener("message", on);
  }, [live, html, origin, round]);
  // A light/dark switch after the page was handed over reaches it without a reload.
  const first = useRef(true);
  useEffect(() => {
    if (first.current) {
      first.current = false;
      return;
    }
    if (handed.current) frame.current?.contentWindow?.postMessage(schemeMessage(scheme), "*");
  }, [scheme]);
  // The top layer follows `full`; Esc closes it, from the page or from the frame.
  useEffect(() => {
    const el = box.current;
    if (!canFull || !el) return;
    const open = el.matches(":popover-open");
    if (full && !open) el.showPopover();
    if (!full && open) el.hidePopover();
    if (!full) return;
    const key = (e: KeyboardEvent) => e.key === "Escape" && setFull(false);
    const msg = (e: MessageEvent) => escapeFrom(e, frame.current?.contentWindow) && setFull(false);
    addEventListener("keydown", key);
    addEventListener("message", msg);
    return () => {
      removeEventListener("keydown", key);
      removeEventListener("message", msg);
    };
  }, [full, live]);
  if (why || origin === null) {
    return (
      <div class="chart-note" data-prototype-note>
        <p class="dim">{why ? `Prototype not shown: ${why.replace(/^prototype: /, "")}` : "Prototypes need a block domain on this page."}</p>
        {why && <pre>{html}</pre>}
      </div>
    );
  }
  if (click && !armed) {
    return (
      <div class="doc-prototype-run" data-prototype-run>
        <span class="doc-prototype-run-title">{title || "Prototype"}</span>
        <span class="dim">Runs the code in this Doc, in a sandbox.</span>
        <button type="button" data-prototype-run-button onClick={() => setArmed(true)}>
          Run prototype
        </button>
      </div>
    );
  }
  if (left) {
    return (
      <div class="doc-prototype-run" data-prototype-left>
        <span>This prototype tried to leave the page. Reload it.</span>
        <button
          type="button"
          data-prototype-reload
          onClick={() => {
            loads.current = 0;
            setLeft(false);
            setRound((n) => n + 1);
          }}
        >
          Reload
        </button>
      </div>
    );
  }
  // Remounted when the text or a reload changes: the page is handed over once.
  return (
    <div class="doc-prototype-box" ref={box} data-prototype-box {...(canFull ? { popover: "manual" } : {})}>
      <span class="doc-prototype-bar">
        {canFull && (
          <button
            type="button"
            class="doc-prototype-full"
            {...(full ? { "data-prototype-full-close": "" } : { "data-prototype-full": "" })}
            aria-label={full ? "Close full window" : "Full window"}
            title={full ? "Close (Esc)" : "Full window"}
            onClick={() => setFull(!full)}
          >
            {full ? <CloseIcon size="xs" /> : <><ExpandIcon size="xs" /> Full window</>}
          </button>
        )}
        <span class="doc-prototype-label" data-prototype-label>
          Prototype · sandboxed
        </span>
      </span>
      {copied && (
        <span class="doc-prototype-copied" data-prototype-copied role="status">
          Copied
        </span>
      )}
      <iframe
        key={`${round}:${html}`}
        ref={frame}
        class="doc-prototype"
        data-prototype-frame
        title={title || "prototype"}
        sandbox={SANDBOX}
        allow=""
        loading="lazy"
        referrerpolicy="no-referrer"
        style={full ? undefined : { height: `${height}px` }}
        onLoad={() => {
          if (++loads.current > OWN_LOADS) {
            setFull(false);
            setLeft(true);
          }
        }}
        {...(origin ? { src: `${origin}/` } : { srcdoc: runner })}
      />
    </div>
  );
}
