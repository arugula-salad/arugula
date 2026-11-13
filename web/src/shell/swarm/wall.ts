// Wall mode (#446): the swarm alone on a TV or a second monitor. Its
// button (or `w`) hides the app's chrome around the swarm (the rail, the
// bar, the swarm's own controls) and asks the browser for fullscreen from
// that press; a refusal still hides the chrome. Escape, its Exit button or
// the browser leaving fullscreen bring everything back as it was. It isn't
// kept: a reload comes back out of it. After a while with no pointer,
// touch or key, the cursor and its buttons hide, never while the pointer
// is on one, and come back with any activity.
//
// It's the swarm container's, not a theme's: the page marks
// `html[data-wall]` and the swarm `[data-wall]` (and `[data-wall-idle]`),
// and style.css does the rest, so every theme and view in the swarm gets it.

import { useEffect, useLayoutEffect, useRef, useState } from "preact/hooks";

/** How long without activity before the cursor and the buttons hide; a
 * test sets `window.__arugulaWallIdleMs` to shorten it. */
const IDLE_MS = 4000;
const idleMs = () => (globalThis as { __arugulaWallIdleMs?: number }).__arugulaWallIdleMs ?? IDLE_MS;

/** What counts as someone there. */
const ACTIVITY = ["pointermove", "pointerdown", "touchstart", "keydown", "wheel"] as const;

type Fs = Document & { webkitFullscreenElement?: Element | null; webkitExitFullscreen?: () => void };
type FsEl = HTMLElement & { webkitRequestFullscreen?: () => void };

const fullscreenElement = () => document.fullscreenElement ?? (document as Fs).webkitFullscreenElement ?? null;

/** Fullscreen for the page, from the press that asked; `given` once it
 * is. Older WebKit has only the prefixed call, which returns nothing (its
 * change event says). A refusal (no API, no permission, a webview without
 * it) is quiet: the chrome hides anyway. */
function askFullscreen(given: () => void) {
  const el = document.documentElement as FsEl;
  try {
    if (el.requestFullscreen) void el.requestFullscreen({ navigationUI: "hide" }).then(given, () => {});
    else el.webkitRequestFullscreen?.();
  } catch {
    // refused: wall mode without it
  }
}

function leaveFullscreen() {
  if (!fullscreenElement()) return;
  const d = document as Fs;
  try {
    if (d.exitFullscreen) void d.exitFullscreen().catch(() => {});
    else d.webkitExitFullscreen?.();
  } catch {
    // already out
  }
}

export interface Wall {
  on: boolean;
  /** No activity for a while: the cursor and the buttons are hidden. */
  idle: boolean;
  enter(): void;
  exit(): void;
  toggle(): void;
  /** The pointer is on a wall button: never idle while it is. */
  hold(on: boolean): void;
}

/** Wall mode for the swarm on the page; `changed` runs after each switch,
 * once the chrome has moved (the scene measures itself again). */
export function useWall(changed: () => void): Wall {
  const [on, setOn] = useState(false);
  const [idle, setIdle] = useState(false);
  const held = useRef(false);
  /** Fullscreen was reached this time: leaving it leaves wall mode. */
  const full = useRef(false);
  const timer = useRef(0);
  const after = useRef(changed);
  after.current = changed;

  const exit = () => {
    full.current = false;
    setOn(false);
    leaveFullscreen();
  };
  const enter = () => {
    full.current = false;
    setOn(true);
    askFullscreen(() => (full.current = true));
  };

  // The page's mark, and leaving fullscreen with it (on the way out too:
  // a pane opened from the wall, or the swarm closed).
  useLayoutEffect(() => {
    if (!on) return;
    document.documentElement.dataset.wall = "";
    const fs = () => {
      if (fullscreenElement()) full.current = true;
      else if (full.current) exit();
    };
    document.addEventListener("fullscreenchange", fs);
    document.addEventListener("webkitfullscreenchange", fs);
    return () => {
      delete document.documentElement.dataset.wall;
      document.removeEventListener("fullscreenchange", fs);
      document.removeEventListener("webkitfullscreenchange", fs);
      leaveFullscreen();
    };
  }, [on]);

  // Measured again once the chrome has gone or come back (fullscreen's
  // own resize comes later, as a resize).
  const first = useRef(true);
  useEffect(() => {
    if (first.current) return void (first.current = false);
    const f = requestAnimationFrame(() => after.current());
    return () => cancelAnimationFrame(f);
  }, [on]);

  // Idle: the cursor and the buttons go after a while without activity.
  useEffect(() => {
    if (!on) return setIdle(false);
    const arm = () => {
      clearTimeout(timer.current);
      timer.current = window.setTimeout(() => {
        if (!held.current) setIdle(true);
      }, idleMs());
    };
    const active = () => {
      setIdle(false);
      arm();
    };
    for (const t of ACTIVITY) addEventListener(t, active, { capture: true, passive: true });
    arm();
    return () => {
      clearTimeout(timer.current);
      for (const t of ACTIVITY) removeEventListener(t, active, { capture: true });
    };
  }, [on]);

  return {
    on,
    idle: on && idle,
    enter,
    exit,
    toggle: () => (on ? exit() : enter()),
    hold: (h) => {
      held.current = h;
      if (h) setIdle(false);
    },
  };
}
