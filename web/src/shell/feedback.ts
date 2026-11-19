// Send feedback's words and its one request (Arugula #1217): what the text
// box says as a title and a body, a refusal's line, and the send through
// this machine's daemon. The dialog is ./ui/feedback.tsx.

import type { Client } from "./client";
import type { FeedbackKind, FeedbackSendRequest, FeedbackSent } from "../shared/proto.gen";

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

/** What the daemon answered, as words. `queued`: #1216's daemon may keep a
 * send for when it's back online (202, or `queued` in its answer). */
export type Outcome = { sent: FeedbackSent; queued: boolean } | { refused: string };

const RATE_LIMITED = "feedback_rate_limited";
const ANONYMOUS_OFF = "feedback_anonymous_off";
const TOO_LARGE = "feedback_too_large";

/** A refusal's one line. */
export function refusalLine(status: number, error: string | undefined, retryAfter?: number | null): string {
  if (error === RATE_LIMITED || status === 429) {
    const mins = retryAfter ? Math.max(1, Math.ceil(retryAfter / 60)) : 0;
    return mins ? `That's a lot of feedback at once. Try again in ${mins === 1 ? "a minute" : `${mins} minutes`}.` : "That's a lot of feedback at once. Try again in a little while.";
  }
  if (error === ANONYMOUS_OFF) return "Arugula isn't taking feedback from machines that aren't signed in right now. Sign in, or join this machine, to send it.";
  if (error === TOO_LARGE || status === 413) return "That's too long to send. Shorten it and try again.";
  if (status === 403) return "Only this machine's owner can send feedback from it.";
  return `Couldn't send it (${error || status}). Try again in a moment.`;
}

/** Send through `client`'s daemon. */
export async function sendFeedback(client: Client, req: FeedbackSendRequest): Promise<Outcome> {
  // Not in OPS yet: #1216 adds `feedback.send` to the daemon's router.
  const res = await client.request("POST", "/api/feedback", req);
  const answer = await res.json<Partial<FeedbackSent> & { queued?: boolean; error?: string; retry_after?: number | null }>().catch(() => null);
  if (res.ok) return { sent: { id: answer?.id ?? "", shown: answer?.shown ?? "" }, queued: res.status === 202 || !!answer?.queued };
  return { refused: refusalLine(res.status, answer?.error, answer?.retry_after) };
}
