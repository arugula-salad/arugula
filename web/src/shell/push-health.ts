// How this device's notifications stand, in words (#468): the settings say
// plainly when this device has no subscription, when notifications aren't
// allowed, when an iPhone isn't running Arugula from its Home Screen, and
// when control has no usable copy of this device's subscription (a
// deploy's push skips it). *Send a test notification* says what became of
// the test. Pure, so `scripts/push-health.test.mjs` reads it.

/** What control says of the account's subscriptions (`POST /api/push/check`,
 * `/api/push/test`): how many, how many the platform's pushes (deploys, the
 * inbox, runs) may go to, and this device's, when the page has one. A
 * test's answer also says what became of it. */
export interface PushStanding {
  subscriptions: number;
  usable: number;
  this_device: "usable" | "unusable" | "missing" | null;
  sent?: number;
  gone?: number;
  failed?: number;
}

/** The words iOS needs before anything else. */
export const HOME_SCREEN = "Add Arugula to your Home Screen to get notifications";

const plural = (n: number, one: string, many = `${one}s`) => `${n} ${n === 1 ? one : many}`;

/** Why this device can't be notified at all, when it can't: `state` is
 * push.ts's `PushState`. */
export function blockerOf(state: string): string | null {
  switch (state) {
    case "insecure":
      return "Notifications need HTTPS";
    case "install":
      return `${HOME_SCREEN} (Share › Add to Home Screen), then open it from there`;
    case "denied":
      return "Notifications are blocked for this site in your browser's settings";
    case "unsupported":
      return "This browser can't show notifications";
    default:
      return null;
  }
}

/** What's wrong with notifications on a device that could have them, or
 * null when nothing is known to be (`blockerOf` says why one can't).
 * `permission` is the browser's `Notification.permission`, when it has one. */
export function healthOf(state: string, standing: PushStanding | null, permission: string | null): string | null {
  if (state === "off")
    return permission === "granted"
      ? "This device has no push subscription: turn on Notify this device."
      : "This device has no push subscription, and notifications haven't been allowed here yet: turn on Notify this device.";
  if (state !== "on" || !standing) return null;
  if (standing.this_device === "missing") return "Control doesn't have this device's subscription, so nothing reaches it: turn Notify this device off and on.";
  if (standing.this_device === "unusable")
    return "This device's subscription is from a key control no longer approves, so deploy and project pushes skip it: turn Notify this device off and on.";
  return null;
}

/** The account's subscriptions, in a line beside the test's button. */
export function standingSaid(s: PushStanding): string {
  if (s.subscriptions === 0) return "Your account has no push subscriptions.";
  const skipped = s.subscriptions - s.usable;
  const all = `Your account has ${plural(s.subscriptions, "push subscription")}`;
  return skipped > 0 ? `${all}; deploy pushes skip ${skipped} of them (a device control no longer approves).` : `${all}.`;
}

/** What became of a test notification. */
export function testSaid(s: PushStanding): string {
  if (s.subscriptions === 0) return "Nothing to send to: your account has no push subscriptions. Turn on Notify this device on each device you want notified.";
  const sent = s.sent ?? 0;
  const parts = [`Sent a test to ${sent} of ${plural(s.subscriptions, "subscription")}`];
  if (s.gone) parts.push(`${s.gone} ${s.gone === 1 ? "was" : "were"} gone and ${s.gone === 1 ? "is" : "are"} forgotten`);
  if (s.failed) parts.push(`${s.failed} failed`);
  const skipped = s.subscriptions - s.usable;
  const tail = skipped > 0 ? ` Deploy pushes skip ${skipped}: turn Notify this device off and on there.` : "";
  return `${parts.join("; ")}.${tail}`;
}
