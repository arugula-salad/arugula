// Send feedback from the shell (Arugula #1217): through this machine's
// daemon (`feedback.send`), which signs it as the joined machine or sends
// it with its proof of work, and fills in the context. The sheet and the
// words are shared/'s (shared/feedback.ts, shared/ui/feedback.tsx).

import type { Client } from "./client";
import { OPS, type FeedbackSendRequest, type FeedbackSent } from "../shared/proto.gen.ts";
import { refusalLine, type Outcome } from "../shared/feedback.ts";

/** Send through `client`'s daemon (`feedback.send`). */
export async function sendFeedback(client: Client, req: FeedbackSendRequest): Promise<Outcome> {
  // Its route has no holes: OPS's own, not through ./ops (opPath).
  const op = OPS["feedback.send"];
  const res = await client.request(op.method, op.path, req);
  const answer = await res.json<Partial<FeedbackSent> & { error?: string; retry_after?: number | null }>().catch(() => null);
  if (res.ok) return { sent: { id: answer?.id ?? "", shown: answer?.shown ?? "", queued: !!answer?.queued } };
  return { refused: refusalLine(res.status, answer?.error, answer?.retry_after) };
}
