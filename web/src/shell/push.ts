// Web Push: let the daemon notify this device when a pane needs you.

import { OPS, opPath } from "./ops";

/** `insecure`: plain http, where browsers have no push. `install`: iOS in
 * a Safari tab, which has push only once added to the Home Screen (#96). */
export type PushState = "unsupported" | "insecure" | "install" | "denied" | "on" | "off";

const supported = () => "serviceWorker" in navigator && "PushManager" in window && "Notification" in window;

/** iOS or iPadOS (which says it's a Mac, but has touch). */
export const ios = () => /iPhone|iPad|iPod/.test(navigator.userAgent) || (/Macintosh/.test(navigator.userAgent) && navigator.maxTouchPoints > 1);

/** Opened from the Home Screen, not a browser tab. */
export const standalone = () =>
  matchMedia("(display-mode: standalone)").matches || (navigator as { standalone?: boolean }).standalone === true;

/** Register the service worker (also what makes the app installable). A
 * notification through control (M21) says which daemon it's from. */
export async function registerWorker(
  onOpenPane: (pane: number, daemon?: string, share?: boolean, follow?: string) => void,
) {
  if (!("serviceWorker" in navigator)) return;
  try {
    await navigator.serviceWorker.register("/sw.js");
    navigator.serviceWorker.addEventListener("message", (e) => {
      if (e.data?.type === "open-pane" && typeof e.data.pane === "number") onOpenPane(e.data.pane, e.data.daemon, e.data.share === true, typeof e.data.follow === "string" ? e.data.follow : undefined);
      // M123: a room in control's chat (sw.ts made the address: `/#chat=…`).
      if (e.data?.type === "open-room" && typeof e.data.url === "string" && /^\/#chat=[a-z]{2}_[0-9a-f]+(?:&msg=-?\d+)?$/.test(e.data.url)) {
        location.hash = e.data.url.slice(2);
      }
      // #104: control's page looks for what waits (its prompt shows).
      if (e.data?.type === "control-refresh") dispatchEvent(new Event("arugula:control-refresh"));
      // M26: at its card on the swarm's rail.
      if (e.data?.type === "open-card" && typeof e.data.pane === "number") {
        location.hash = `swarm=${e.data.daemon ? `${e.data.daemon}.` : ""}${e.data.pane}`;
      }
    });
  } catch {
    // Not a secure context (plain http on the tailnet): no worker, no push.
  }
}

export async function pushState(): Promise<PushState> {
  if (backend?.state) return backend.state();
  if (!isSecureContext) return "insecure";
  if (!supported()) return ios() && !standalone() ? "install" : "unsupported";
  if (Notification.permission === "denied") return "denied";
  const reg = await navigator.serviceWorker.getRegistration();
  const sub = await reg?.pushManager.getSubscription();
  return sub ? "on" : "off";
}

function key(b64url: string): Uint8Array<ArrayBuffer> {
  const b64 = b64url.replace(/-/g, "+").replace(/_/g, "/") + "=".repeat((4 - (b64url.length % 4)) % 4);
  return Uint8Array.from(atob(b64), (c) => c.charCodeAt(0));
}

/** What control pushes an account about (`/api/push/prefs`). */
export interface PushPrefs {
  chat: boolean;
  machines: boolean;
  projects: boolean;
}

/** In control mode, subscribing goes to control instead (M21), and the
 * account chooses what it's pushed about. In the desktop app, which has no
 * Web Push, the app shows what control pushes while it runs instead
 * (`desktopPush`): its own `state`, and a `note` that says when. */
export interface PushBackend {
  enable(): Promise<PushState>;
  disable(): Promise<PushState>;
  prefs(): Promise<PushPrefs>;
  setPrefs(p: Partial<PushPrefs>): Promise<PushPrefs>;
  state?(): Promise<PushState>;
  note?: string;
}
let backend: PushBackend | null = null;
const backendChanged = new Set<() => void>();
export function setPushBackend(b: PushBackend | null) {
  backend = b;
  backendChanged.forEach((fn) => fn());
}
/** Hear when another backend takes over (the desktop app's, once it answers). */
export function onPushBackend(fn: () => void) {
  backendChanged.add(fn);
}

/** What control pushes about, or null off control. */
export const pushPrefs = () => backend?.prefs() ?? Promise.resolve(null);
export const setPushPrefs = (p: Partial<PushPrefs>) => backend?.setPrefs(p) ?? Promise.resolve(null);
/** When this device is notified, if not always ("while the app is open"). */
export const pushNote = () => backend?.note ?? null;

/** The desktop app's notifications (crates/desktop/src/notices.rs): the app
 * makes the subscription's keys and keeps them, control takes it at its own
 * live endpoint (signed by this device, as any subscription is) and answers
 * the token the app listens with, and the app holds the stream open while
 * it runs. */
export function desktopPush(
  invoke: <T>(cmd: string, args?: Record<string, unknown>) => Promise<T>,
  control: {
    subscribe(sub: { endpoint: string; p256dh: string; auth: string }): Promise<{ listen?: string }>;
    unsubscribe(endpoint: string): Promise<unknown>;
    prefs(): Promise<PushPrefs>;
    setPrefs(p: Partial<PushPrefs>): Promise<PushPrefs>;
  },
): PushBackend {
  return {
    note: "while the app is open",
    state: async () => ((await invoke<string>("notices_state")) === "on" ? "on" : "off"),
    enable: async () => {
      const k = await invoke<{ id: string; p256dh: string; auth: string }>("notices_keys");
      const endpoint = `${location.origin}/api/push/live/${k.id}`;
      const { listen } = await control.subscribe({ endpoint, p256dh: k.p256dh, auth: k.auth });
      if (!listen) throw new Error("control didn't take the app's subscription");
      await invoke("notices_start", { endpoint, listen });
      return "on";
    },
    disable: async () => {
      const endpoint = await invoke<string | null>("notices_stop");
      if (endpoint) await control.unsubscribe(endpoint).catch(() => {});
      return "off";
    },
    prefs: () => control.prefs(),
    setPrefs: (p) => control.setPrefs(p),
  };
}

export async function enablePush(): Promise<PushState> {
  if (backend) return backend.enable();
  if (!supported()) return "unsupported";
  if ((await Notification.requestPermission()) !== "granted") return "denied";
  const reg = (await navigator.serviceWorker.getRegistration()) ?? (await navigator.serviceWorker.register("/sw.js"));
  await navigator.serviceWorker.ready;
  const res = await fetch(opPath("push.key"));
  if (!res.ok) throw new Error("this daemon has push turned off");
  const { key: k } = (await res.json()) as { key: string };
  const sub = await reg.pushManager.subscribe({ userVisibleOnly: true, applicationServerKey: key(k) });
  const saved = await fetch(opPath("push.subscribe"), {
    method: OPS["push.subscribe"].method,
    headers: { "content-type": "application/json" },
    body: JSON.stringify(sub.toJSON()),
  });
  if (!saved.ok) throw new Error(`subscribing failed: ${saved.status}`);
  return "on";
}

/** Control mode (M21): subscribe once with control's key, and sign the
 * subscription with this device's key so control can't swap it. */
export async function enableControlPush(vapid: string, sign: (sub: { endpoint: string; p256dh: string; auth: string }) => Promise<unknown>): Promise<PushState> {
  if (!supported()) return "unsupported";
  if ((await Notification.requestPermission()) !== "granted") return "denied";
  const reg = (await navigator.serviceWorker.getRegistration()) ?? (await navigator.serviceWorker.register("/sw.js"));
  await navigator.serviceWorker.ready;
  const sub = await reg.pushManager.subscribe({ userVisibleOnly: true, applicationServerKey: key(vapid) });
  const j = sub.toJSON() as { endpoint: string; keys: { p256dh: string; auth: string } };
  await sign({ endpoint: j.endpoint, p256dh: j.keys.p256dh, auth: j.keys.auth });
  return "on";
}

export async function disablePush(): Promise<PushState> {
  if (backend) return backend.disable();
  const reg = await navigator.serviceWorker.getRegistration();
  await (await reg?.pushManager.getSubscription())?.unsubscribe();
  return "off";
}
