// What control's trust screens (this folder, M149) draw with: following
// the control session, the centred page and the QR code, the overlay's
// dialogs opened by name (the `arugula:control-panel` event), taking a
// link out of the address bar, and how long ago something was.

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

export { openPanel, type Panel } from "../ui/control-panel";

export function clearHash() {
  usedLink = "";
  history.replaceState(null, "", location.pathname + location.search);
  dispatchEvent(new HashChangeEvent("hashchange"));
}

/** Where a dialog the phone app opened in its web sheet (`&app` on its
 * address, Arugula #604) goes when it's done: the app's own scheme, which
 * closes the sheet and hands the app back. Always this one address, never
 * one the link names. */
export const APP_BACK = "arugula://back";

/** Back to the phone app that opened this dialog. */
export function backToApp() {
  clearHash();
  // For a test, which can't follow the app's scheme.
  document.documentElement.dataset.backToApp = APP_BACK;
  location.assign(APP_BACK);
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
