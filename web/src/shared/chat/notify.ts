// How loud chat is, and do not disturb (#113, #115), as the page uses
// them: the account's settings (`/api/notify/prefs`, the platform's), the
// snooze presets, and the words for a level. One copy of the settings for
// the room's bell and /settings, so snoozing in one shows in the other.

import type { NotifyLevel, NotifyPrefs, NotifySet } from "../proto.gen";
import { platformFetch, refusalOf } from "../maintenance";

export const LEVELS: { level: NotifyLevel; label: string; hint: string }[] = [
  { level: "all", label: "All messages", hint: "Every message in it" },
  { level: "mentions", label: "Mentions only", hint: "When someone @-mentions you, and in DMs" },
  { level: "nothing", label: "Nothing", hint: "No notifications; mentions still count" },
];

export const levelLabel = (l: NotifyLevel) => LEVELS.find((x) => x.level === l)?.label ?? l;

let prefs: NotifyPrefs | null = null;
let asked: Promise<void> | null = null;
let timer: number | undefined;
const listeners = new Set<() => void>();

const emit = () => {
  listeners.forEach((fn) => fn());
  // The moon goes out by itself when a snooze ends.
  clearTimeout(timer);
  if (prefs?.snooze_until) timer = window.setTimeout(() => void loadNotify(true), Math.min(prefs.snooze_until - Date.now() + 500, 2 ** 31 - 1));
};

export const notifyPrefs = () => prefs;
export function subscribeNotify(fn: () => void) {
  listeners.add(fn);
  return () => void listeners.delete(fn);
}

async function call(method: string, body?: unknown): Promise<NotifyPrefs> {
  const res = await platformFetch("/api/notify/prefs", {
    method,
    headers: body === undefined ? {} : { "Content-Type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
    credentials: "same-origin",
  });
  const j = (await res.json().catch(() => ({}))) as NotifyPrefs & { error?: string };
  if (!res.ok) throw new Error(refusalOf(res.status, j, `HTTP ${res.status}`));
  return j;
}

/** Ask once (or again, with `again`); the settings are `notifyPrefs()` after. */
export function loadNotify(again = false): Promise<void> {
  if (!asked || again) {
    asked = call("GET").then(
      (p) => {
        prefs = p;
        emit();
      },
      () => {
        asked = null;
      },
    );
  }
  return asked;
}

/** Change some of them. */
export async function setNotify(patch: NotifySet): Promise<void> {
  prefs = await call("POST", patch);
  emit();
}

export * from "./notify-time";
