// "Finish in your browser" (#236): a settings flow that leaves for another
// site and finishes on the server (the GitHub App's install, Stripe
// Checkout). In the desktop app the site opens in the person's browser and
// the window would wait on nothing; this step opens it there, says so, and
// asks again when the window gets the person back. A plain browser keeps its
// same-tab redirect, so callers use this only when `desktopApp()`.
//
// An app with the `open_outside` command (#364) needs no click for it, so the
// step opens the site itself as soon as it has the address and goes straight
// to waiting (#381): the button that brought the person here already says
// ↗. An app without it keeps the open button, whose click is the gesture
// `window.open` needs.

import { useEffect, useLayoutEffect, useRef, useState } from "preact/hooks";
import { desktopApp, openOutside, openOutsideAsync, tauriInvoke } from "../desktop";

/** The label of a button that leaves for the browser: marked as such in the
 * desktop app only. */
export const browserButton = (label: string) => (desktopApp() ? { children: `${label} ↗`, title: "Opens in your browser" } : { children: label });

/**
 * - `url`: where to go, or a getter for one that takes a request (checkout's).
 *   A getter runs when the step appears, not on the click: the click then
 *   opens a known address, still inside the person's gesture (WKWebView
 *   treats a `window.open` after an `await` as a popup).
 * - `label`: the button that opens it, for an app that can't open it itself.
 * - `check`: true once what happened there shows here. It runs when the
 *   window regains focus or becomes visible, and on *Check again*; no timer.
 * - `onDone`: after `check` said true; the step goes away.
 * - `cancel`: the person gave up.
 */
export function BrowserStep({
  url,
  label,
  check,
  onDone,
  cancel,
}: {
  url: string | (() => Promise<string>);
  label: string;
  check: () => Promise<boolean>;
  onDone: () => void;
  cancel: () => void;
}) {
  const [href, setHref] = useState(typeof url === "string" ? url : "");
  const [opened, setOpened] = useState(false);
  // The app is opening it (#381): until it says it did, or couldn't.
  const [trying, setTrying] = useState(() => !!tauriInvoke());
  const tried = useRef(false);
  const [checking, setChecking] = useState(false);
  const [err, setErr] = useState("");
  const running = useRef(false);
  const rerun = useRef(false);
  const live = useRef(true);
  // The latest callbacks, for listeners set once.
  const latest = useRef({ check, onDone });
  latest.current = { check, onDone };

  useEffect(() => {
    live.current = true;
    if (typeof url !== "string") url().then((u) => live.current && setHref(u), (e: Error) => live.current && (setErr(e.message), setTrying(false)));
    return () => void (live.current = false);
  }, []);

  // Once per step, when the address is known: never with an empty one, and
  // not again if it changes or the step redraws.
  useEffect(() => {
    if (!trying || !href || tried.current) return;
    tried.current = true;
    void openOutsideAsync(href).then((ok) => {
      if (!live.current) return;
      if (ok) setOpened(true);
      setTrying(false);
    });
  }, [href, trying]);

  const again = async () => {
    // Back again while one is out: it may have read the answer before the
    // change, so ask once more when it lands.
    if (running.current) {
      rerun.current = true;
      return;
    }
    running.current = true;
    setChecking(true);
    setErr("");
    try {
      if (await latest.current.check()) {
        live.current = false;
        latest.current.onDone();
      }
    } catch (e) {
      if (live.current) setErr((e as Error).message);
    } finally {
      running.current = false;
      if (live.current) setChecking(false);
      if (rerun.current && live.current) {
        rerun.current = false;
        void again();
      }
    }
  };

  // Only once the browser has been opened: before that nothing has left.
  // Set before the waiting line paints, so a return can't beat it.
  useLayoutEffect(() => {
    if (!opened) return;
    const seen = () => document.visibilityState === "visible" && void again();
    window.addEventListener("focus", again);
    document.addEventListener("visibilitychange", seen);
    return () => {
      window.removeEventListener("focus", again);
      document.removeEventListener("visibilitychange", seen);
    };
  }, [opened]);

  return (
    <div class="browser-step" data-browser-step>
      {opened ? (
        <>
          <p data-browser-step-waiting>Finish in your browser, then come back here.</p>
          <div class="prompt-buttons">
            <button
              type="button"
              class="secondary"
              data-browser-step-again-open
              title="Opens in your browser"
              onClick={() => {
                // For a tab closed by mistake, or a browser that didn't come up.
                openOutside(href) || window.open(href, "_blank", "noopener");
              }}
            >
              Open again ↗
            </button>
            <button type="button" class="secondary" data-browser-step-again disabled={checking} onClick={() => void again()}>
              Check again
            </button>
            <button type="button" class="secondary" data-browser-step-cancel onClick={cancel}>
              Cancel
            </button>
          </div>
        </>
      ) : trying ? (
        <>
          <p class="dim" data-browser-step-opening>
            Opening your browser…
          </p>
          <div class="prompt-buttons">
            <button type="button" class="secondary" data-browser-step-cancel onClick={cancel}>
              Cancel
            </button>
          </div>
        </>
      ) : (
        <div class="prompt-buttons">
          <button
            type="button"
            class="primary"
            data-browser-step-open
            disabled={!href}
            {...browserButton(label)}
            onClick={() => {
              // The app opens it in the browser itself (#364): its
              // `window.open` keeps control's own pages in an app window.
              openOutside(href) || window.open(href, "_blank", "noopener");
              setOpened(true);
            }}
          />
          <button type="button" class="secondary" data-browser-step-cancel onClick={cancel}>
            Cancel
          </button>
        </div>
      )}
      {err && <p class="control-error">{err}</p>}
    </div>
  );
}
