// Send feedback (Arugula #1217): what a person writes, sent through this
// machine's daemon (`POST /api/feedback`, the `feedback.send` operation,
// #1216). The daemon signs it as the joined machine, or sends it
// anonymously with its proof of work, and fills in the context (version,
// OS, the page): the page never talks to control for it. Opened from the
// account menu and the command palette's Help; nothing opens it by itself.
//
// One text box: its first line is the title, the rest the body. On a phone
// the dialog is a sheet along the bottom (style.css, `.feedback`).

import { useEffect, useState } from "preact/hooks";
import type { Client } from "../client";
import type { FeedbackKind, FeedbackSendRequest } from "../../shared/proto.gen";
import { KINDS, MAX_BODY, sendFeedback, splitFeedback, type Outcome } from "../feedback";
import { Modal } from "../../shared/ui/modal";
import { usePhone } from "../../shared/ui/hooks";
import { desktopApp } from "../../shared/desktop";
import { getSignedIn } from "../signed-in";

/** Whether an account is signed in on this page (control's page, the app,
 * the desktop app signed in to control). A daemon's own page has none. */
const accountSignedIn = () => getSignedIn()?.phase === "ready" || !!desktopApp()?.signedIn;

let shownFor: Client | null = null;
const listeners = new Set<() => void>();
const changed = () => listeners.forEach((fn) => fn());

/** Open Send feedback, sending through `client`'s daemon. */
export function openFeedback(client: Client) {
  shownFor = client;
  changed();
}

export function FeedbackLayer() {
  const [, tick] = useState(0);
  useEffect(() => {
    const fn = () => tick((t) => t + 1);
    listeners.add(fn);
    return () => void listeners.delete(fn);
  }, []);
  if (!shownFor) return null;
  const close = () => {
    shownFor = null;
    changed();
  };
  return <FeedbackDialog key={shownFor.base} client={shownFor} close={close} />;
}

function FeedbackDialog({ client, close }: { client: Client; close: () => void }) {
  const phone = usePhone();
  const signedIn = accountSignedIn();
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
    const req: FeedbackSendRequest = {
      kind,
      title,
      body,
      contact_ok: signedIn && contact,
      source: "person",
      surface: desktopApp() ? "desktop" : "web",
      // The page's path only: a query or fragment could carry a token.
      route: location.pathname,
    };
    const out = await sendFeedback(client, req).catch((err: unknown) => ({ refused: `Couldn't reach this machine (${err instanceof Error ? err.message : String(err)}). Try again in a moment.` }));
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
