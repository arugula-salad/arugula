// Send feedback's words (Arugula #1217, #1240): what the text box says as
// a title and a body, a refusal's line, and control's own send and answer
// for a page signed in to it. The sheet is ./ui/feedback.tsx; who sends is
// the caller's: the shell through this machine's daemon (shell/feedback.ts),
// the platform pages straight to control (platform/feedback.ts).

import type { Arch, FeedbackAck, FeedbackContext, FeedbackKind, FeedbackRefused, FeedbackSendRequest, FeedbackSent, FeedbackSubmit, Os } from "./proto.gen";

/** The chips, in the order they're shown. */
export const KINDS: [FeedbackKind, string][] = [
  ["bug", "Bug"],
  ["missing", "Missing"],
  ["idea", "Idea"],
  ["praise", "Love it"],
];

/** control-wire's limits (feedback.rs): the title's characters and the
 * body's bytes. */
const MAX_TITLE = 200;
export const MAX_BODY = 8 * 1024;

/** What one text box says as a title and a body: the first line that has
 * anything on it is the title, the rest the body. A first line too long for
 * a title is cut there, and kept whole in the body, so nothing is lost. */
export function splitFeedback(text: string): { title: string; body: string } {
  const lines = text.replace(/\r\n?/g, "\n").split("\n");
  const at = lines.findIndex((l) => l.trim() !== "");
  if (at === -1) return { title: "", body: "" };
  const first = lines[at].trim();
  const rest = lines
    .slice(at + 1)
    .join("\n")
    .trim();
  const chars = [...first];
  if (chars.length <= MAX_TITLE) return { title: first, body: rest };
  return { title: `${chars.slice(0, MAX_TITLE - 1).join("")}…`, body: rest ? `${first}\n\n${rest}` : first };
}

/** What the daemon answered: what it sent (or queued, `sent.queued`), or
 * the line to show for a refusal. */
export type Outcome = { sent: FeedbackSent } | { refused: string };

const RATE_LIMITED = "feedback_rate_limited";
const ANONYMOUS_OFF = "feedback_anonymous_off";
const TOO_LARGE = "feedback_too_large";

/** A refusal's one line. The daemon says why in a sentence of its own
 * (`refused_words`), shown as it is; a bare code or status gets ours. */
export function refusalLine(status: number, error: string | undefined, retryAfter?: number | null): string {
  if (error && !/^feedback_\w+$/.test(error)) return error;
  if (error === RATE_LIMITED || status === 429) {
    const mins = retryAfter ? Math.max(1, Math.ceil(retryAfter / 60)) : 0;
    return mins ? `That's a lot of feedback at once. Try again in ${mins === 1 ? "a minute" : `${mins} minutes`}.` : "That's a lot of feedback at once. Try again in a little while.";
  }
  if (error === ANONYMOUS_OFF) return "Arugula isn't taking feedback from machines that aren't signed in right now. Sign in, or join this machine, to send it.";
  if (error === TOO_LARGE || status === 413) return "That's too long to send. Shorten it and try again.";
  if (status === 403) return "Only this machine's owner can send feedback from it.";
  return `Couldn't send it (${error || status}). Try again in a moment.`;
}

/** What the person wrote in the sheet; whoever sends it adds where from. */
export type FeedbackWritten = Pick<FeedbackSendRequest, "kind" | "title" | "body" | "contact_ok">;

/** Sends what was written, and says how it went. */
export type FeedbackSender = (written: FeedbackWritten) => Promise<Outcome>;

/** The page's path: its query or fragment could carry a token. */
export const pagePath = (): string => location.pathname;

/** The OS a browser says it's on, as control's names; null when it
 * doesn't say one of them. */
export function browserOs(platform: string): Os | null {
  const p = platform.toLowerCase();
  if (/mac|iphone|ipad|ios/.test(p)) return "macos";
  if (/win/.test(p)) return "windows";
  if (/linux|android|x11|cros/.test(p)) return "linux";
  return null;
}

/** The processor a browser says (`userAgentData`'s high-entropy `arch`),
 * when it says one plainly; else null. */
export function browserArch(arch: string | undefined): Arch | null {
  if (arch === "arm") return "aarch64";
  if (arch === "x86") return "x86_64";
  return null;
}

/** A page signed in to control: what it sends to `POST /api/feedback`
 * (control-wire's `FEEDBACK`) with its session. Who it's from is the
 * session's, so `who` is empty: no install id, no stamp. */
export function controlSubmit(w: FeedbackWritten, context: Pick<FeedbackContext, "version" | "os" | "arch" | "route">): FeedbackSubmit {
  return {
    ...w,
    source: "person",
    surface: "web",
    agent: null,
    context: { version: context.version, os: context.os, arch: context.arch, onboarding: null, route: context.route, failed: null },
    who: { install: null, stamp: null },
  };
}

/** Control's answer to a send, as an outcome: its `FeedbackAck`, or its
 * `FeedbackRefused`'s code as our line. */
export function controlOutcome(status: number, ok: boolean, answer: Partial<FeedbackAck & FeedbackRefused> | null): Outcome {
  if (ok) return { sent: { id: answer?.id ?? "", shown: answer?.shown ?? "", queued: false } satisfies FeedbackSent };
  if (status === 401) return { refused: "You've been signed out of this page. Sign in again to send it." };
  return { refused: refusalLine(status, answer?.error, answer?.retry_after) };
}
