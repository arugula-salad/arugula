// The passkey reset (DK10, #393, aru-119 as amended): the passkeys alone,
// for a person who lost them. The device reset that made a browser the
// account's first device went with device approvals (aru-141, #1004).

import { useEffect, useState } from "preact/hooks";
import { agreeWith, api, type ControlSession, type PasskeyRemoval } from "../control";
import { signText } from "../e2e/keys.ts";
import { Time } from "../../shared/ui/time";

interface ResetState {
  pending: { started_at: number; due: number; mine?: boolean } | null;
  done: boolean;
  /** While one waits: what one of the account's passkeys signs to cancel
   * it (DK10). */
  cancel?: PasskeyRemoval | null;
}

// ---------------------------------------------------------------- passkeys
//
// The passkey reset (DK10, #393, aru-119 as amended): the passkeys alone,
// for a person who lost them while their devices are in use. Removing a
// passkey takes another, so it's the way back from lost ones.
//
// - `PasskeyResetOffer`, under Passkeys in *Sign-in and account*, where the
//   "last passkey" and "can't remove" refusals point: what it does, the
//   start (a fresh GitHub sign-in), then the wait with Cancel.
// - `PasskeyResetWarning`, in *Machines* on a browser that holds a device
//   key: one is waiting, with "This wasn't me: cancel", signed by that key.

interface PasskeyResetState extends ResetState {
  /** While one waits: what a passkey signs to cancel it. */
  cancel?: PasskeyRemoval | null;
}

/** The text a device signs to cancel (control's `reset::passkey_cancel_body`). */
const passkeyCancelBody = (account: string, startedAt: number) => `arugula passkey reset cancel v1\n${account}\n${startedAt}`;

function usePasskeyReset() {
  const [r, setR] = useState<PasskeyResetState | null>(null);
  const load = () => api<PasskeyResetState>("/api/me/passkeys/reset").then(setR, () => {});
  useEffect(() => void load(), []);
  return [r, load] as const;
}

/** `done`: the passkeys were reset, so the list changed. `back`: where a
 * fresh GitHub sign-in comes back to. */
export function PasskeyResetOffer({ done, back }: { done: () => void; back: string }) {
  const [r, load] = usePasskeyReset();
  const [open, setOpen] = useState(false);
  const [err, setErr] = useState("");
  useEffect(() => {
    if (r?.done) done();
  }, [r?.done]);
  if (!r) return null;
  const run = async (f: () => Promise<unknown>) => {
    setErr("");
    try {
      await f();
      await load();
    } catch (e) {
      const why = (e as { body?: { reason?: string } }).body?.reason;
      // A GitHub sign-in from the last ten minutes: through GitHub, then here.
      if (why === "reauth") location.assign(`/auth/github?reauth=true&next=${encodeURIComponent(`/${back}`)}`);
      else setErr((e as Error).message);
    }
  };
  if (r.pending) {
    const cancel = r.cancel;
    return (
      <div class="control-aside" data-passkey-reset-pending={r.pending.mine ? "mine" : "other"}>
        <p>
          <b>Your passkeys reset</b> <Time at={r.pending.due} on />: every passkey is removed, and you add one again. Your email has been told, and any of your
          passkeys can cancel it.
        </p>
        {r.pending.mine ? (
          <button class="secondary" data-passkey-reset-cancel onClick={() => void run(() => api("/api/me/passkeys/reset/cancel", {}))}>
            Cancel the reset
          </button>
        ) : cancel && cancel.allowCredentials.length ? (
          <button
            class="secondary"
            data-passkey-reset-cancel-passkey
            onClick={() => void run(async () => api("/api/me/passkeys/reset/cancel", { passkey: await agreeWith(cancel) }))}
          >
            Cancel it with a passkey
          </button>
        ) : null}
        {err ? <p class="control-error">{err}</p> : null}
      </div>
    );
  }
  return (
    <div class="control-aside" data-passkey-reset-offer>
      {!open ? (
        <button class="link" data-passkey-reset-open onClick={() => setOpen(true)}>
          Lost your passkeys? Start a passkey reset
        </button>
      ) : (
        <>
          <p>
            <b>Reset your passkeys.</b> After a 48-hour wait, every passkey of yours is removed and you can add one again. Your machines stay as they are.
          </p>
          <p class="dim">Your email is told when it starts, and any of your passkeys can cancel it. It needs a GitHub sign-in from the last ten minutes.</p>
          <button class="secondary danger" data-passkey-reset-start onClick={() => void run(() => api("/api/me/passkeys/reset", {}))}>
            Reset my passkeys in 48 hours
          </button>
        </>
      )}
      {err ? <p class="control-error">{err}</p> : null}
    </div>
  );
}

export function PasskeyResetWarning({ s }: { s: ControlSession }) {
  const [r, load] = usePasskeyReset();
  const [err, setErr] = useState("");
  if (!r?.pending) return null;
  const p = r.pending;
  const cancel = async () => {
    setErr("");
    try {
      const sig = await signText(s.keys, passkeyCancelBody(s.account, p.started_at));
      await api("/api/me/passkeys/reset/cancel", { device: s.keys.id, sig });
      await load();
    } catch (e) {
      setErr((e as Error).message);
    }
  };
  return (
    <div class="control-dropped" role="alert" data-passkey-reset-warning>
      <p>
        <b>A reset of your passkeys is waiting.</b> It started <Time at={p.started_at} on /> from a GitHub sign-in, and happens <Time at={p.due} on />:
        every passkey is removed. If it wasn't you, cancel it.
      </p>
      <div class="control-dropped-actions">
        <button class="primary" data-passkey-reset-warning-cancel onClick={() => void cancel()}>
          This wasn't me: cancel
        </button>
      </div>
      {err ? <p class="control-error">{err}</p> : null}
    </div>
  );
}
