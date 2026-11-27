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

import { createContext } from "preact";
import { useContext, useEffect, useLayoutEffect, useRef, useState } from "preact/hooks";
import { useScheme } from "./drawing";
import { checkPrototype, prototypeInfo } from "./embeds";
import { ProtoFiles, ProtoScope, useBridge } from "./prototype-bridge";
import { protoTarget, type ProtoFile } from "./prototype-files";
import { escapeFrom, htmlMessage, navFrom, readyFrom, schemeMessage } from "./prototype-messages";
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

/** Loads of a frame's window after its page is handed over that are the
 * runner's own: the one `document.close()` fires when it has written the
 * prototype. A second is the prototype navigating itself.
 *
 * Counted per window (Arugula #869). Moving the iframe in the DOM (a phone
 * parks a hidden block off the page and moves it back in when it's shown)
 * drops its window and loads the runner again in a new one, which is handed
 * the page again; a navigation keeps the window, so its load is counted. A
 * window is handed the page once both have reached here: its load and its
 * runner saying it's ready, in either order (they come on different task
 * sources: Chrome's load comes first). Its loads before that aren't counted.
 * And a window is handed it once: a later "ready" from it is the prototype's,
 * not the runner's. */
export const OWN_LOADS = 1;

/** How long a load past the runner's own waits to say where it went. */
const LEAVE_GRACE_MS = 600;

/** Full window rides on the top layer (#507): the box itself is a manual
 * popover, so the same iframe is drawn over the window without moving in the
 * DOM, which would reload it and lose its state. `position: fixed` wouldn't do:
 * the pane area is CSS-scaled, and a transform makes `fixed` relative to it. */
const canFull = typeof HTMLElement !== "undefined" && "showPopover" in HTMLElement.prototype;

/** Set by a prototype Doc's view (Arugula #830): the frame fills the box it
 * is in, whose height the view sets, and not the fence's `height=`. */
export const ProtoFill = createContext(false);

/** A reader's click to run, kept per Doc and section (`ProtoScope`) so a new
 * version of the section (#509) doesn't ask again. */
const armedKeys = new Set<string>();

export function Prototype({ html: own, alt }: { html: string; alt: string }) {
  const { origin, click } = useMode();
  const scheme = useScheme();
  const frame = useRef<HTMLIFrameElement>(null);
  /** The frame's window that has the page, and its loads since. */
  const handed = useRef<Window | null>(null);
  const loads = useRef(0);
  /** A window's load, and its runner's ready, before the handover. */
  const loaded = useRef<Window | null>(null);
  const ready = useRef<Window | null>(null);
  const handOver = useRef<(win: Window) => void>(() => {});
  const schemeNow = useRef(scheme);
  schemeNow.current = scheme;
  const scope = useContext(ProtoScope);
  const fill = useContext(ProtoFill);
  const armKey = scope ? `${scope.doc}/${scope.section}` : null;
  const [armed, arm] = useState(() => armKey !== null && armedKeys.has(armKey));
  const setArmed = (on: boolean) => {
    if (on && armKey !== null) armedKeys.add(armKey);
    arm(on);
  };
  const files = useContext(ProtoFiles);
  const filesNow = useRef(files);
  filesNow.current = files;
  // Another of the Doc's prototypes this one went to (#1451), until its own
  // text changes.
  const [went, setWent] = useState<{ from: string; to: ProtoFile } | null>(null);
  const swapped = went && went.from === own ? went.to : null;
  const html = swapped?.html ?? own;
  /** A load past the runner's own, until the page says where it went. */
  const leaving = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const [left, setLeft] = useState(false);
  const [round, setRound] = useState(0);
  const [full, setFull] = useState(false);
  const box = useRef<HTMLDivElement>(null);
  const { title: ownTitle, height } = prototypeInfo(alt);
  const title = swapped?.title ?? ownTitle;
  const why = checkPrototype(html);
  const live = !why && origin !== null && (!click || armed) && !left;
  const copied = useBridge(frame, live, swapped?.section);
  // A layout effect: the listener is there before the frame loads, after
  // which the runner says it is ready.
  useLayoutEffect(() => {
    if (!live) return;
    handed.current = loaded.current = ready.current = null;
    loads.current = 0;
    handOver.current = (win) => {
      handed.current = win;
      loads.current = 0;
      loaded.current = ready.current = null;
      // An opaque origin has no name to target: "*", for this one message.
      win.postMessage(htmlMessage(html, schemeNow.current), "*");
    };
    // The runner says it's ready in each window it loads in (a new one each
    // time the frame is moved): handed the page once that window's load is here too.
    const on = (e: MessageEvent) => {
      const win = frame.current?.contentWindow;
      if (!win || !readyFrom(e, win) || win === handed.current) return;
      if (loaded.current === win) handOver.current(win);
      else ready.current = win;
    };
    addEventListener("message", on);
    return () => removeEventListener("message", on);
  }, [live, html, origin, round]);
  // Where the page says it is going (a link, `location`): another of the
  // Doc's prototypes is shown here instead; anything else is leaving.
  useEffect(() => {
    if (!live) return;
    const on = (e: MessageEvent) => {
      const url = navFrom(e, frame.current?.contentWindow);
      if (url === null) return;
      clearTimeout(leaving.current);
      const to = protoTarget(url, filesNow.current);
      if (to) setWent({ from: own, to });
      else {
        setFull(false);
        setLeft(true);
      }
    };
    addEventListener("message", on);
    return () => {
      removeEventListener("message", on);
      clearTimeout(leaving.current);
    };
  }, [live, own]);
  // A light/dark switch after the page was handed over reaches it without a reload.
  const first = useRef(true);
  useEffect(() => {
    if (first.current) {
      first.current = false;
      return;
    }
    handed.current?.postMessage(schemeMessage(scheme), "*");
  }, [scheme]);
  // The top layer follows `full`; Esc closes it, from the page or from the frame.
  useEffect(() => {
    const el = box.current;
    if (!canFull || !el) return;
    const open = el.matches(":popover-open");
    if (full && !open) el.showPopover();
    if (!full && open) el.hidePopover();
    if (!full) return;
    // Taken: not a step back too (shared/kit/frame/stack.ts).
    const key = (e: KeyboardEvent) => e.key === "Escape" && (e.preventDefault(), setFull(false));
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
    <div class={fill ? "doc-prototype-box doc-prototype-fill" : "doc-prototype-box"} ref={box} data-prototype-box {...(canFull ? { popover: "manual" } : {})}>
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
        // Not `loading="lazy"` (Arugula #947): WebKit loads a lazy frame that
        // was put in off the page (a phone's parked block) and then moved into
        // view twice in the same window, and the second load, after the
        // handover, would count as the prototype leaving.
        referrerpolicy="no-referrer"
        style={full || fill ? undefined : { height: `${height}px` }}
        onLoad={() => {
          const win = frame.current?.contentWindow;
          if (!win) return;
          if (win === handed.current) {
            // A write to `location` can't be caught in the frame: what it
            // loads from the daemon's own origin says where it went (#1451).
            // Given a moment to; a page that says nothing has left.
            if (++loads.current > OWN_LOADS) {
              clearTimeout(leaving.current);
              leaving.current = setTimeout(() => {
                setFull(false);
                setLeft(true);
              }, LEAVE_GRACE_MS);
            }
            return;
          }
          // A window that hasn't had the page: the runner's first, or the
          // frame was moved and the runner loaded again in a new one.
          handed.current = null;
          loaded.current = win;
          if (ready.current === win) handOver.current(win);
        }}
        {...(origin ? { src: `${origin}/` } : { srcdoc: runner })}
      />
    </div>
  );
}
