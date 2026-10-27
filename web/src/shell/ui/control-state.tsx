// #325: this machine's standing with Arugula control, from `/api/host`'s
// `control_state` (crates/daemon/src/control.rs): joined (where, connected
// or not), not joined, or dropped by control. Dropped is a banner under the
// top bar with Join again (Getting started's join); the host menu has a
// line for it either way. A machine whose key was removed (#330) asks to
// join again by itself: the banner shows that join's code. In the desktop
// app, whose window comes back to this page when control drops the machine
// (cloud.rs), the join itself opens, once per drop (#326): that's what the
// person came to do. Only the
// owner's own daemon says (not a guest, not a page from control), and it's
// asked again every half minute, so a drop shows within a minute or two of
// control's.

import { opPath } from "../ops";
import { useEffect, useState } from "preact/hooks";
import type { Client } from "../client";
import { desktopApp } from "../../shared/desktop";
import type { ControlState } from "../../shared/proto";
import { useSubscribe } from "../../shared/ui/hooks";
import { Time } from "../../shared/ui/time";
import type { MenuItem } from "../../shared/ui/menu";
import { openGettingStarted } from "./welcome";

const POLL_MS = 30 * 1000;
const HIDDEN_KEY = "arugula.control-dropped.hidden";
const SIGNIN_HIDDEN_KEY = "arugula.app-signin.hidden";
const OPENED_KEY = "arugula.control-dropped.opened";

let now: ControlState | null = null;
const listeners = new Set<() => void>();
const changed = () => listeners.forEach((fn) => fn());

export function controlState(): ControlState | null {
  return now;
}

export function subscribeControlState(fn: () => void): () => void {
  listeners.add(fn);
  return () => void listeners.delete(fn);
}

/** Set it from a `/api/host` answer fetched elsewhere. */
export function setControlState(s: ControlState | null | undefined) {
  const next = s ?? null;
  if (JSON.stringify(next) === JSON.stringify(now)) return;
  now = next;
  changed();
}

async function fetchState() {
  const r = await fetch(opPath("host.get")).catch(() => null);
  if (!r?.ok) return;
  const host = (await r.json().catch(() => null)) as { control_state?: ControlState } | null;
  // An older daemon, or not the owner: nothing to say.
  if (host) setControlState(host.control_state);
}

/** Ask again now, and once the daemon has picked up a join (it looks every
 * few seconds). */
export function refreshControlState() {
  void fetchState();
  setTimeout(() => void fetchState(), 5000);
}

/** Keep asking this page's daemon while it's the owner's own. */
export function useControlState(client: Client) {
  const mine = !client.e2e && !client.state?.roles;
  useEffect(() => {
    if (!mine) return;
    void fetchState();
    const t = setInterval(() => void fetchState(), POLL_MS);
    const focus = () => void fetchState();
    window.addEventListener("focus", focus);
    return () => {
      clearInterval(t);
      window.removeEventListener("focus", focus);
    };
  }, [mine]);
  useSubscribe(subscribeControlState);
  return mine ? now : null;
}

function hostOf(url?: string): string {
  if (!url) return "control";
  try {
    return new URL(url).host;
  } catch {
    return url;
  }
}

/** "the team arugula on control.example", "lex00's account on …". */
export function placeOf(s: ControlState): string {
  const on = hostOf(s.url);
  if (s.kind === "team") return s.name ? `the team ${s.name} on ${on}` : `a team on ${on}`;
  return s.name ? `${s.name}'s account on ${on}` : `an account on ${on}`;
}

/** The line the host menu (and the tray, and `arugula status`) says. */
export function controlLine(s: ControlState): string {
  switch (s.state) {
    case "joined":
      return s.connected ? `In ${placeOf(s)}: connected` : s.error ? `In ${placeOf(s)}: not connected` : `In ${placeOf(s)}: connecting`;
    case "dropped":
      return `Dropped by control: no longer in ${placeOf(s)}`;
    default:
      return "Not joined to Arugula control";
  }
}

const joinAgain = () => openGettingStarted("cloud");

/** The host menu's control line, and what to do about it. */
export function controlMenuItems(): MenuItem[] {
  const s = now;
  if (!s) return [];
  const why = s.state === "joined" && !s.connected && s.error ? ` (${s.error})` : "";
  const line: MenuItem = { label: controlLine(s) + why, disabled: true, run: () => {} };
  if (s.state === "dropped") return ["separator", line, { label: "Join again…", run: joinAgain }];
  if (s.state === "not_joined") return ["separator", line, { label: "Join…", run: joinAgain }];
  return ["separator", line];
}

function hiddenFor(): string | null {
  try {
    return sessionStorage.getItem(HIDDEN_KEY);
  } catch {
    return null;
  }
}

/** #1580: in the desktop app, this machine is in control but the app isn't
 * signed in there (it never was, it picked "Just this machine", or control
 * moved), so this window has no chat and no other machines. Says so, with
 * Sign the app in: control's own sign-in, which the app takes over
 * (`is_control_signin` in crates/desktop/src/cloud.rs). "Not now" hides it
 * for this tab. */
function AppSignInBanner({ s }: { s: ControlState }) {
  const [, redraw] = useState(0);
  let hidden = false;
  try {
    hidden = sessionStorage.getItem(SIGNIN_HIDDEN_KEY) === s.url;
  } catch {
    // Shown each time; harmless.
  }
  if (hidden || !s.url) return null;
  const control = s.url.replace(/\/+$/, "");
  const hide = () => {
    try {
      sessionStorage.setItem(SIGNIN_HIDDEN_KEY, s.url ?? "");
    } catch {
      // Nothing to remember it in.
    }
    redraw((n) => n + 1);
  };
  return (
    <div class="control-dropped app-signin" role="status" data-app-signin>
      <p>
        <b>This window shows only this machine.</b> It's in {placeOf(s)}, but the app isn't signed in there, so chat and your other machines aren't here.
      </p>
      <div class="control-dropped-actions">
        <button class="primary" onClick={() => location.assign(`${control}/auth/github`)} data-app-signin-go>
          Sign the app in
        </button>
        <button onClick={hide} data-app-signin-hide>
          Not now
        </button>
      </div>
    </div>
  );
}

/** Dropped by control: says so, with Join again. "Not now" hides it for
 * this tab until control says it again (another drop). In the desktop app,
 * joined but not signed in: AppSignInBanner. */
export function ControlBanner({ client }: { client: Client }) {
  const s = useControlState(client);
  const drop = s?.state === "dropped" ? String(s.dropped_ms ?? "") : null;
  useEffect(() => {
    if (drop === null || !desktopApp()) return;
    try {
      if (localStorage.getItem(OPENED_KEY) === drop) return;
      localStorage.setItem(OPENED_KEY, drop);
    } catch {
      // Nothing to remember it in: the banner's Join again is there.
      return;
    }
    joinAgain();
  }, [drop]);
  if (s?.state === "joined" && desktopApp()?.signedIn === false) return <AppSignInBanner s={s} />;
  if (!s || s.state !== "dropped") return null;
  const key = String(s.dropped_ms ?? "");
  if (hiddenFor() === key) return null;
  const hide = () => {
    try {
      sessionStorage.setItem(HIDDEN_KEY, key);
    } catch {
      // Shown again on the next poll; harmless.
    }
    changed();
  };
  return (
    <div class="control-dropped" role="alert" data-control-dropped>
      <p>
        <b>This machine is no longer in {placeOf(s)}.</b> Control dropped it
        {s.dropped_ms ? (
          <>
            {" "}
            <Time at={s.dropped_ms} on />
          </>
        ) : (
          ""
        )}
        : it left, or was removed.
        {s.said ? <span class="control-dropped-said"> Control says: {s.said}</span> : null}
        {s.code ? (
          <span data-control-dropped-code>
            {" "}
            It asked to join again: approve <b>{s.code}</b> on a device you use.
          </span>
        ) : null}
      </p>
      <div class="control-dropped-actions">
        <button class="primary" onClick={joinAgain} data-control-rejoin>
          Join again
        </button>
        <button onClick={hide} data-control-dropped-hide>
          Not now
        </button>
      </div>
    </div>
  );
}
