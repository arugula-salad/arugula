// What control's trust screens (this folder, M149) draw with: following
// the control session, the centred page and the QR code, the overlay's
// dialogs opened by name (the `arugula:control-panel` event), taking a
// link out of the address bar, and how long ago something was.

import { useSubscribe } from "../../shared/ui/hooks";
import { qr, qrPath } from "../../shared/ui/qr";
import type { ControlSession } from "../control";

export function useControl(s: ControlSession) {
  useSubscribe((fn) => s.subscribe(fn));
}

export function Center({ children }: { children: preact.ComponentChildren }) {
  return <div class="control-center">{children}</div>;
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

export const since = (ms: number) => {
  const s = Math.max(0, Math.round((Date.now() - ms) / 1000));
  return s < 60 ? "just now" : s < 3600 ? `${Math.round(s / 60)}m ago` : s < 86400 ? `${Math.round(s / 3600)}h ago` : `${Math.round(s / 86400)}d ago`;
};

export type Panel = "cloud" | "devices" | "add" | "add-device" | "teams" | "plan" | "account" | "shared-docs";

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

/** When a device was added, to the minute: with the last use beside it,
 * what tells apart two devices of the same name (#759). */
export const addedAt = (ms: number) =>
  new Date(ms).toLocaleString(undefined, { month: "short", day: "numeric", ...(new Date(ms).getFullYear() === new Date().getFullYear() ? {} : { year: "numeric" }), hour: "numeric", minute: "2-digit" });
