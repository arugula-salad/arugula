// The account's email for security notices (#1609): what control keeps
// (GitHub's primary verified address, or one typed and verified by a
// link), a link waiting, and changing or taking it off. Only notices go
// there: a reset of your devices, and the like. Control's Sign-in and
// account dialog (shell/trust/email.tsx) and the app's Settings › You ›
// Sign-in and security draw it, each sending `/api/me/email` its own way.
// The desktop app's window shows it, and changes happen in a browser
// (control refuses the window): `readOnly`.

import { useEffect, useState } from "preact/hooks";
import type { AccountCall } from "../account";

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

export function EmailSection({ call, back, readOnly, heading = true }: { call: AccountCall; back?: string; readOnly?: boolean; heading?: boolean }) {
  const [e, setE] = useState<EmailState | null>(null);
  const [typing, setTyping] = useState(false);
  const [draft, setDraft] = useState("");
  const [said, setSaid] = useState(back ? BACK[back] : "");
  const [err, setErr] = useState("");
  const load = () => call<EmailState>("GET", "/api/me/email").then(setE, (x: Error) => setErr(x.message));
  useEffect(() => void load(), []);
  if (!e) return err ? <p class="control-error">{err}</p> : null;
  const send = async () => {
    setErr("");
    try {
      await call("POST", "/api/me/email", { email: draft.trim() });
      setTyping(false);
      setSaid(`A link went to ${draft.trim()}: open it within a day.`);
      await load();
    } catch (x) {
      setErr((x as Error).message);
    }
  };
  const remove = async () => {
    setErr("");
    await call("DELETE", "/api/me/email").catch((x: Error) => setErr(x.message));
    setSaid("");
    await load();
  };
  const where = e.source === "github" ? "from GitHub" : "typed here";
  return (
    <>
      {heading ? <h3 class="control-group">Email</h3> : null}
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
              ? "this server doesn't send email"
              : e.email && e.verified
                ? `security notices go here, ${where}`
                : "add one so a reset of your devices reaches you"}
          </span>
          {readOnly || !e.sends ? (
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
