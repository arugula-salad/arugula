// Send feedback (Arugula #1217, #1240): one sheet for the shell and the
// platform pages. What a person writes goes to the `send` it was opened
// with: the shell's sends through this machine's daemon (shell/ui/feedback.tsx),
// the platform's straight to control with its session (platform/feedback.ts).
// Opened from an account menu or the command palette; nothing opens it by
// itself.
//
// One text box: its first line is the title, the rest the body. On a phone
// the dialog is a sheet along the bottom (style.css, `.feedback`).

import { useEffect, useState } from "preact/hooks";
import type { FeedbackKind } from "../proto.gen";
import { KINDS, MAX_BODY, splitFeedback, type FeedbackSender, type Outcome } from "../feedback";
import { Modal } from "./modal";
import { usePhone } from "./hooks";

/** Who sends, and whether an account is signed in (the contact box). */
export interface FeedbackFrom {
  send: FeedbackSender;
  signedIn: boolean;
}

let shown: FeedbackFrom | null = null;
let opened = 0;
const listeners = new Set<() => void>();
const changed = () => listeners.forEach((fn) => fn());

/** Open Send feedback, sending through `from.send`. */
export function openFeedbackSheet(from: FeedbackFrom) {
  shown = from;
  opened++;
  changed();
}

export function FeedbackLayer() {
  const [, tick] = useState(0);
  useEffect(() => {
    const fn = () => tick((t) => t + 1);
    listeners.add(fn);
    return () => void listeners.delete(fn);
  }, []);
  if (!shown) return null;
  const close = () => {
    shown = null;
    changed();
  };
  return <FeedbackDialog key={opened} from={shown} close={close} />;
}

function FeedbackDialog({ from, close }: { from: FeedbackFrom; close: () => void }) {
  const phone = usePhone();
  const { signedIn } = from;
  const [text, setText] = useState("");
  const [kind, setKind] = useState<FeedbackKind | null>(null);
  const [contact, setContact] = useState(signedIn);
  const [busy, setBusy] = useState(false);
  const [outcome, setOutcome] = useState<Outcome | null>(null);
  const { title, body } = splitFeedback(text);
  const tooLong = new TextEncoder().encode(body).length > MAX_BODY;

  const send = async (e: Event) => {
    e.preventDefault();
    if (busy || !kind || !title || tooLong) return;
    setBusy(true);
    const out = await from
      .send({ kind, title, body, contact_ok: signedIn && contact })
      .catch((err: unknown): Outcome => ({ refused: `Couldn't send it (${err instanceof Error ? err.message : String(err)}). Try again in a moment.` }));
    setBusy(false);
    setOutcome(out);
  };

  const cls = phone ? "feedback phone" : "feedback";
  if (outcome && "sent" in outcome) {
    const { sent } = outcome;
    const queued = !!sent.queued;
    return (
      <Modal
        class={cls}
        title="Send feedback"
        close={close}
        root={{ "data-feedback": "sent" }}
        buttons={
          <button class="primary" type="button" data-autofocus onClick={close}>
            Done
          </button>
        }
      >
        <p class="feedback-done" data-feedback-shown={sent.shown || undefined}>
          {sent.shown ? `Thanks — ${queued ? "saved" : "sent"} as ${sent.shown}.` : `Thanks — ${queued ? "saved" : "sent"}.`}
          {queued ? " It goes when this machine is back online." : ""}
        </p>
      </Modal>
    );
  }

  return (
    <Modal
      class={cls}
      title="Send feedback"
      close={close}
      onSubmit={(e) => void send(e)}
      root={{ "data-feedback": "write" }}
      buttons={
        <button class="primary" type="submit" disabled={busy || !kind || !title || tooLong} aria-busy={busy}>
          Send
        </button>
      }
    >
      <div class="seg feedback-kinds" role="group" aria-label="What it is">
        {KINDS.map(([k, label]) => (
          <button key={k} type="button" aria-pressed={kind === k} onClick={() => setKind(k)}>
            {label}
          </button>
        ))}
      </div>
      <textarea
        class="feedback-text"
        aria-label="Your feedback"
        placeholder={"What happened, or what you'd like\nThe first line is its title."}
        rows={6}
        data-autofocus
        value={text}
        onInput={(e) => setText((e.currentTarget as HTMLTextAreaElement).value)}
        onKeyDown={(e) => {
          // ⌘/Ctrl-Enter sends, as a composer does; Enter is a new line.
          if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) void send(e);
        }}
      />
      {tooLong && (
        <p class="feedback-line error" role="alert">
          That's longer than 8 KB. Shorten it to send.
        </p>
      )}
      {signedIn && (
        <label class="feedback-contact">
          <input type="checkbox" checked={contact} onChange={(e) => setContact((e.currentTarget as HTMLInputElement).checked)} />
          You can reach me about this
        </label>
      )}
      <p class="feedback-line dim">Sends your Arugula version, OS and this page. Never your terminal.</p>
      {outcome && "refused" in outcome && (
        <p class="feedback-line error" role="alert" data-feedback-refused>
          {outcome.refused}
        </p>
      )}
    </Modal>
  );
}
