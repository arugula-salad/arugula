// Send feedback from the platform pages (Arugula #1240): the shared sheet
// (shared/ui/feedback.tsx), sending straight to control's `POST
// /api/feedback` with this page's session. No daemon and no proof of work:
// who it's from is the session's.

import type { FeedbackAck, FeedbackRefused } from "../shared/proto.gen";
import { browserArch, browserOs, controlOutcome, controlSubmit, pagePath, type FeedbackWritten, type Outcome } from "../shared/feedback";
import { MAINTENANCE, MAINTENANCE_SAID, platformFetch } from "../shared/maintenance";
import { openFeedbackSheet } from "../shared/ui/feedback";

/** This build's version (vite.platform.config.ts), when it was built with one. */
const VERSION = (import.meta.env.VITE_ARUGULA_VERSION as string | undefined) ?? null;

type UAData = { platform?: string; getHighEntropyValues?: (hints: string[]) => Promise<{ architecture?: string }> };

/** The OS and processor, as far as this browser says. */
async function where(): Promise<{ os: ReturnType<typeof browserOs>; arch: ReturnType<typeof browserArch> }> {
  const ua = (navigator as Navigator & { userAgentData?: UAData }).userAgentData;
  const os = browserOs(ua?.platform || navigator.platform || "");
  const arch = await ua
    ?.getHighEntropyValues?.(["architecture"])
    .then((v) => browserArch(v.architecture))
    .catch(() => null);
  return { os, arch: arch ?? null };
}

async function send(w: FeedbackWritten): Promise<Outcome> {
  const { os, arch } = await where();
  const body = JSON.stringify(controlSubmit(w, { version: VERSION, os, arch, route: pagePath() }));
  const res = await platformFetch("/api/feedback", { method: "POST", credentials: "same-origin", headers: { "Content-Type": "application/json" }, body });
  const answer = (await res.json().catch(() => null)) as (Partial<FeedbackAck & FeedbackRefused> & { reason?: string }) | null;
  // A window platformFetch waited out to its end.
  if (res.status === 503 && answer?.reason === MAINTENANCE) return { refused: MAINTENANCE_SAID };
  return controlOutcome(res.status, res.ok, answer);
}

/** Open Send feedback, sending to control as the person signed in. */
export function openPlatformFeedback() {
  openFeedbackSheet({ signedIn: true, send });
}
