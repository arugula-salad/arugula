// What "Confirm the new passkey" says is being added (#471): since DK11 a
// passkey the account has is the only check on another, so the person
// confirming sees what they vouch for. Its own module so the unit tests
// (scripts/passkey.test.mjs) run it without the page.

import { ago } from "../../shared/time.ts";

/** A passkey whose AAGUID control doesn't know (or that won't say). */
export const UNKNOWN_PROVIDER = "an unknown passkey provider";

/** "Adding: 1Password passkey, made just now in Chrome on Linux.": what
 * keeps it (control's word from its AAGUID, else unknown), when it was
 * made, and the browser that made it, in the words Sign-in will name it
 * by when control doesn't know its provider. */
export function addingPasskey(provider: string | null | undefined, browser: string, made: number, now = Date.now()): string {
  const what = provider ? `${provider} passkey` : `a passkey from ${UNKNOWN_PROVIDER}`;
  const where = browser.replace(/^An? /, (a) => a.toLowerCase());
  return `Adding: ${what}, made ${ago(made, now, "long")} in ${where}.`;
}
