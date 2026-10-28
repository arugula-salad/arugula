// The passkey hint's fact: an account with no passkey (GitHub signs it in)
// is signed in on control, which can make passkeys (it has a domain name),
// on a device that can make one (a platform authenticator: Touch ID,
// Windows Hello, a phone). Asked of the browser once a page.

import type { Client } from "../client";
import type { ControlSession } from "../control";
import type { Fact } from "./index";
import { factChanged } from "./facts";
import { getControlSession } from "../ui/people";

const watched = new WeakSet<ControlSession>();
/** Whether this device can make one: unknown until the browser says. */
let able: boolean | "asking" | undefined;

export function passkeyWanted(_client: Client): Fact | null {
  const s = getControlSession();
  if (!s) return null;
  if (!watched.has(s)) {
    watched.add(s);
    s.subscribe(factChanged);
  }
  if (s.phase !== "ready" || !s.info.passkeys || s.passkeys > 0) return null;
  if (able === undefined) {
    able = "asking";
    const ask = window.PublicKeyCredential?.isUserVerifyingPlatformAuthenticatorAvailable?.() ?? Promise.resolve(false);
    void ask
      .catch(() => false)
      .then((yes) => {
        able = yes;
        factChanged();
      });
  }
  return able === true ? { args: {} } : null;
}
