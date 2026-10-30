// Live presence's switch (Arugula #341, the `live-presence` Labs flag): Off,
// Watching or Live, in the header of control's page and the platform page,
// and in the account menu's "Your presence".
//
// Off and Watching are the account's, kept by control beside chat's
// presence mode (../chat/presence), so every page and window agrees. Live
// is Watching plus this window's camera: it's this window's alone, and off
// again when the page reloads. What follows from it (joining the rooms,
// the camera) is the live presence store's (#343); this only says what the
// person chose.

import { useRef } from "preact/hooks";
import { presence } from "../chat/presence";
import { useSubscribe } from "../ui/hooks";
import { openMenu, useMenuFrom, type MenuItem } from "../ui/menu";
import "./switch.css";

export type LiveState = "off" | "watching" | "live";

/** What arugula.stream sees, in a line (Who else sees it says it in full). */
export const LIVE_TIP =
  "Live presence: your teammates' faces, live, on their avatars. Watching tells arugula.stream you're around (your account, name and teams); Live also sends your character's movements from your camera, never a picture.";

/** This window's camera, on top of the account's Watching. */
let here = false;
const listeners = new Set<() => void>();

function changed() {
  for (const fn of [...listeners]) fn();
}

export function subscribeLive(fn: () => void): () => void {
  listeners.add(fn);
  const off = presence.subscribe(fn);
  return () => {
    listeners.delete(fn);
    off();
  };
}

/** The reader's live presence here, or null with the flag off (control
 * says nothing of it) or chat off. */
export function liveState(): LiveState | null {
  if (!presence.on || presence.live === null) return null;
  if (presence.live === "off") return "off";
  return here ? "live" : "watching";
}

/** Choose Off, Watching or Live. Live keeps the account on Watching and
 * turns this window's camera on; the others turn it off here. */
export async function chooseLive(s: LiveState) {
  here = s === "live";
  changed();
  const kept = s === "off" ? "off" : "watching";
  if (presence.live !== kept) await presence.setLive(kept);
}

const LABELS: Record<LiveState, string> = { off: "Off", watching: "Watching", live: "Live (my camera)" };

/** The three, for a menu: the header's, and the account menu's. */
export function liveItems(withHeader = true): MenuItem[] {
  const now = liveState();
  if (now === null) return [];
  const item = (s: LiveState): MenuItem => ({ label: LABELS[s], checked: now === s, hint: LIVE_TIP, run: () => void chooseLive(s) });
  return [...(withHeader ? [{ header: "Live presence" } as MenuItem] : []), item("off"), item("watching"), item("live")];
}

const CameraIcon = () => (
  <svg class="icon" width={18} height={18} viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
    <rect x="3" y="6.5" width="12.5" height="11" rx="2" />
    <path d="M15.5 10.5l5-3v9l-5-3z" />
  </svg>
);

/** The header's control: a camera, dim at Off, plain while Watching, lit
 * (with a dot) while this window's camera is Live. Nothing with the flag
 * off. */
export function LiveSwitch() {
  useSubscribe(subscribeLive);
  const button = useRef<HTMLButtonElement>(null);
  const open = useMenuFrom(button);
  const s = liveState();
  if (s === null) return null;
  const said = s === "live" ? "Live: your camera is on in this window" : s === "watching" ? "Watching" : "Off";
  const pick = (e: MouseEvent) => {
    const at = (e.currentTarget as HTMLElement).getBoundingClientRect();
    openMenu({ clientX: at.right, clientY: at.bottom + 4, preventDefault: () => e.preventDefault() }, liveItems(), e.currentTarget as HTMLElement);
  };
  return (
    <button
      ref={button}
      class={`ws-live ws-live-${s}${open ? " on" : ""}`}
      data-live-switch={s}
      aria-label={`Live presence: ${said}`}
      aria-haspopup="menu"
      aria-expanded={open}
      title={`Live presence: ${said}. ${LIVE_TIP}`}
      onClick={pick}
    >
      <CameraIcon />
      {s === "live" && <span class="ws-live-dot" aria-hidden="true" />}
    </button>
  );
}
