// The account's email for security notices (#1609), in Sign-in and
// account: what control keeps (GitHub's primary verified address, or one
// typed and verified by a link), a link waiting, and changing or taking it
// off. Only notices go there: a reset of your devices, and the like. The
// desktop app's window shows it, and changes happen in a browser (control
// refuses the window).

import { useEffect, useState } from "preact/hooks";
import { api } from "../control";
import { desktopApp } from "../../shared/desktop";

/** Where the verify link comes back to: `#email=verified` or `#email=expired`. */
export const EMAIL_HASH = /^#email=(verified|expired)$/;

interface EmailState {
  email: string | null;
  source: "github" | "typed" | null;
  verified: boolean;
  pending: string | null;
  sends: boolean;
}

const BACK: Record<string, string> = {
  verified: "Your address is verified: security notices go there now.",
  expired: "That link expired or was used already: send a new one.",
};

export function EmailSection({ back }: { back?: string }) {
  const [e, setE] = useState<EmailState | null>(null);
  const [typing, setTyping] = useState(false);
  const [draft, setDraft] = useState("");
  const [said, setSaid] = useState(back ? BACK[back] : "");
  const [err, setErr] = useState("");
  const load = () => api<EmailState>("/api/me/email").then(setE, (x: Error) => setErr(x.message));
  useEffect(() => void load(), []);
  if (!e) return null;
  const app = !!desktopApp();
  const send = async () => {
    setErr("");
    try {
      await api("/api/me/email", { email: draft.trim() });
      setTyping(false);
      setSaid(`A link went to ${draft.trim()}: open it within a day.`);
      await load();
    } catch (x) {
      setErr((x as Error).message);
    }
  };
  const remove = async () => {
    setErr("");
    const r = await fetch("/api/me/email", { method: "DELETE" });
    if (!r.ok) setErr(`HTTP ${r.status}`);
    setSaid("");
    await load();
  };
  const where = e.source === "github" ? "from GitHub" : "typed here";
  return (
    <>
      <h3 class="control-group">Email</h3>
      {said ? (
        <p class={back === "expired" ? "control-error" : "dim"} data-email-said>
          {said}
        </p>
      ) : null}
      <ul class="control-devices control-rows" data-email>
        <li>
          <span data-email-address>{e.email ?? "No email"}</span>
          <span class="dim">
            {!e.sends
              ? "this control doesn't send email"
              : e.email && e.verified
                ? `security notices go here, ${where}`
                : "add one so a reset of your devices reaches you"}
          </span>
          {app || !e.sends ? (
            <span />
          ) : (
            <span class="control-row-actions">
              <button class="secondary small" data-email-change onClick={() => setTyping(!typing)}>
                {e.email ? "Change" : "Add"}
              </button>
              {e.email ? (
                <button class="secondary danger small" data-email-remove onClick={() => void remove()}>
                  Remove
                </button>
              ) : null}
            </span>
          )}
        </li>
      </ul>
      {e.pending ? (
        <p class="dim" data-email-pending>
          A link went to {e.pending}: open it within a day to use that address.
        </p>
      ) : null}
      {typing ? (
        <form
          class="control-email-form"
          onSubmit={(ev) => {
            ev.preventDefault();
            void send();
          }}
        >
          <input type="email" aria-label="Email address" required value={draft} onInput={(ev) => setDraft((ev.target as HTMLInputElement).value)} data-email-input />
          <button class="primary small" type="submit" data-email-send>
            Send a link
          </button>
        </form>
      ) : null}
      {err ? <p class="control-error">{err}</p> : null}
    </>
  );
}
