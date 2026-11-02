// A prototype in a Doc (#506): the page in a ```prototype section, run in a
// sandboxed frame. The frame is `sandbox="allow-scripts"` and nothing else, so
// its origin is opaque: no cookies or storage, no reaching the Doc's page, the
// machine or the account, and no navigating the window above it. Inside, a CSP
// (`runner.html`) lets it run its own inline scripts and styles and loads
// nothing, not even a fetch.
//
// Where the page has no script CSP (the daemon's own app), the frame is the
// runner page by `srcdoc`. A srcdoc frame inherits its parent's CSP, so where the
// page has one (control's platform pages and the shared viewer: `script-src
// 'self'`) the inline scripts would never run: the frame loads the same page
// from control's proto origin instead, which serves it under a CSP of its
// own and which those pages' `frame-src` names. With no proto origin there (no
// block domain) the section says so.
//
// On the shared viewer, where the reader is a stranger, nothing is framed until
// they click "Run prototype". In the Doc block and on project Docs it runs at once.

import { useEffect, useLayoutEffect, useRef, useState } from "preact/hooks";
import { useScheme } from "./drawing";
import { checkPrototype, prototypeInfo } from "./embeds";
import { htmlMessage, readyFrom } from "./prototype-messages";
import runner from "./runner.html?raw";

/** The frame's whole sandbox: scripts, and nothing that would give it a way out. */
export const SANDBOX = "allow-scripts";

interface Mode {
  /** Where the runner loads from: `undefined`, by `srcdoc`; a proto origin
   * (`https://proto.<block domain>`); `null`, nowhere (the page can't run one). */
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

export function Prototype({ html, alt }: { html: string; alt: string }) {
  const { origin, click } = useMode();
  const scheme = useScheme();
  const frame = useRef<HTMLIFrameElement>(null);
  const loads = useRef(0);
  const [armed, setArmed] = useState(false);
  const [left, setLeft] = useState(false);
  const [round, setRound] = useState(0);
  const { title, height } = prototypeInfo(alt);
  const why = checkPrototype(html);
  const live = !why && origin !== null && (!click || armed) && !left;
  // A layout effect: the listener is there before the frame's first task, in
  // which the runner says it is ready.
  useLayoutEffect(() => {
    if (!live) return;
    // The first time the frame says it is ready, and no other.
    let sent = false;
    const on = (e: MessageEvent) => {
      if (sent || !readyFrom(e, frame.current?.contentWindow)) return;
      sent = true;
      // An opaque origin has no name to target: "*", for this one message.
      frame.current!.contentWindow!.postMessage(htmlMessage(html, scheme), "*");
    };
    addEventListener("message", on);
    return () => removeEventListener("message", on);
  }, [live, html, scheme, origin, round]);
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
  // Remounted when the text, the scheme or a reload changes: the page is handed over once.
  return (
    <div class="doc-prototype-box">
      <span class="doc-prototype-label" data-prototype-label>
        Prototype · sandboxed
      </span>
      <iframe
        key={`${round}:${scheme}:${html}`}
        ref={frame}
        class="doc-prototype"
        data-prototype-frame
        title={title || "prototype"}
        sandbox={SANDBOX}
        allow=""
        loading="lazy"
        referrerpolicy="no-referrer"
        style={{ height: `${height}px` }}
        onLoad={() => {
          if (++loads.current > OWN_LOADS) setLeft(true);
        }}
        {...(origin ? { src: `${origin}/` } : { srcdoc: runner })}
      />
    </div>
  );
}
