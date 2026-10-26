// What control's trust screens (this folder, M149) draw with: following
// the control session, the centred page and the QR code, the overlay's
// dialogs opened by name (the `arugula:control-panel` event), taking a
// link out of the address bar, and how long ago something was.

import { useEffect, useState } from "preact/hooks";
import { useSubscribe } from "../../shared/ui/hooks";
import { qr, qrPath } from "../../shared/ui/qr";
import { ago } from "../../shared/time";
import { Time } from "../../shared/ui/time";
import type { ControlSession } from "../control";
import { MovingNote } from "../ui/moving-note";

export function useControl(s: ControlSession) {
  useSubscribe((fn) => s.subscribe(fn));
}

/** A gate screen: centred while it fits the window, scrolling when it
 * doesn't (UI audit #148). */
export function Center({ children }: { children: preact.ComponentChildren }) {
  return (
    <div class="control-gate">
      <div class="control-center">
        <MovingNote />
        {children}
      </div>
    </div>
  );
}

/** A waiting screen's one line (UI audit #179): that it's waiting, live,
 * and for how long, rather than a dim "Waiting…" or three. */
export function WaitingLine({ what = "Waiting for approval" }: { what?: string }) {
  const [from] = useState(() => Date.now());
  const [now, setNow] = useState(from);
  useEffect(() => {
    const t = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(t);
  }, []);
  const s = Math.floor((now - from) / 1000);
  const took = `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
  return (
    <p class="control-waiting" role="status" data-waiting-line>
      <span class="control-waiting-dot" aria-hidden="true" />
      {what} <span class="dim">· {took}</span>
    </p>
  );
}

/** A link as a QR code, for a phone's camera. */
export function QrCode({ text, label }: { text: string; label: string }) {
  const code = qrPath(qr(text));
  return (
    <svg class="control-qr" viewBox={`0 0 ${code.n} ${code.n}`} role="img" aria-label={label} data-qr={text}>
      <rect width={code.n} height={code.n} fill="#fff" />
      <path d={code.d} fill="#000" />
    </svg>
  );
}

/** "2d ago", or "on Oct 16" (../../shared/time.ts), in a sentence. */
export const since = (ms: number) => ago(Math.min(ms, Date.now()), Date.now(), "on");

export type Panel = "devices" | "add" | "add-device" | "teams" | "account" | "shared-docs";

export const openPanel = (p: Panel) => () => dispatchEvent(new CustomEvent("arugula:control-panel", { detail: p }));

export function clearHash() {
  usedLink = "";
  history.replaceState(null, "", location.pathname + location.search);
  dispatchEvent(new HashChangeEvent("hashchange"));
}

/** An invite link that was used: out of the address bar, so a reload
 * doesn't offer it again (#208), but its prompt stays up until closed,
 * through the overlay mounting afresh. */
export let usedLink = "";

export function dropHash() {
  if (location.hash) usedLink = location.hash;
  history.replaceState(null, "", location.pathname + location.search);
}

/** When a device was added: with the last use beside it, what tells
 * apart two devices of the same name (#759); the minute is in its title
 * (UI audit #182). */
export const AddedAt = ({ at }: { at: number }) => <Time at={Math.min(at, Date.now())} on data-added={at} />;
