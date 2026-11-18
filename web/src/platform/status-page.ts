// The status page (Arugula #1176): status.arugula.io says whether hosted
// Arugula is up, so the page links it, in your picture's menu and in
// Settings › You, only when it is hosted Arugula's page: when its hub
// (`X-Arugula-Hub`, session.ts's `controlOrigin`) is the hosted control.
// The hub says, never a guess from this page's host name. A control you
// run yourself has no status page of ours, so no link. No imports, so
// scripts/status-page.test.mjs runs it under node.

/** The hosted control, the CLI's default (`ssh::CONTROL`). */
export const HOSTED_CONTROL = "https://control.arugula.io";

/** Whether hosted Arugula is up, and any incident. */
export const STATUS_PAGE = "https://status.arugula.io";

/** The status page to link on a page whose hub is `hub`, or null. */
export function statusPage(hub: string): string | null {
  return hub.replace(/\/+$/, "") === HOSTED_CONTROL ? STATUS_PAGE : null;
}

/** Open it in a new tab that can't reach back into this one. */
export function openStatusPage(url: string) {
  window.open(url, "_blank", "noopener");
}
