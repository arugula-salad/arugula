// Resetting your own devices (aru-119, #1582,
// docs/design/self-service-reset.md).
//
// - `ResetOffer`, on *Approve this browser*: when nothing can approve, a
//   way out that says what it costs, takes a fresh GitHub sign-in, and
//   then shows the wait with Cancel. Once it's due the page reloads and
//   this browser enrolls as the first device.
// - `ResetWarning`, in *Devices and machines* on a device the account
//   trusts: a reset is waiting, with "This wasn't me: cancel", signed by
//   this device's key so a signed-in session alone can't.

import { useEffect, useState } from "preact/hooks";
import { agreeWith, api, type ControlSession, type PasskeyRemoval } from "../control";
import { signText } from "../e2e/keys.ts";
import { Time } from "../../shared/ui/time";

/** The fragment the GitHub sign-in comes back to, to carry on. */
export const RESET_HASH = "#reset";

interface ResetState {
  pending: { started_at: number; due: number; mine?: boolean } | null;
  done: boolean;
}

/** The text a device signs to cancel (control's `reset::cancel_body`). */
const cancelBody = (account: string, startedAt: number) => `arugula reset cancel v1\n${account}\n${startedAt}`;

function useReset(poll: boolean) {
  const [r, setR] = useState<ResetState | null>(null);
  const load = () => api<ResetState>("/api/me/reset").then(setR, () => {});
  useEffect(() => {
    void load();
    if (!poll) return;
    const t = setInterval(() => void load(), 60_000);
    return () => clearInterval(t);
  }, [poll]);
  return [r, load] as const;
}

export function ResetOffer() {
  const [r, load] = useReset(true);
  const [open, setOpen] = useState(location.hash === RESET_HASH);
  const [err, setErr] = useState("");
  useEffect(() => {
    if (r?.done) location.reload();
  }, [r?.done]);
  if (!r) return null;
  const start = async () => {
    setErr("");
    try {
      await api("/api/me/reset", {});
      if (location.hash === RESET_HASH) history.replaceState(history.state, "", location.pathname + location.search);
      await load();
    } catch (e) {
      const why = (e as { body?: { reason?: string } }).body?.reason;
      // A GitHub sign-in from the last ten minutes: through GitHub, then here.
      if (why === "reauth") location.assign(`/auth/github?reauth=true&next=${encodeURIComponent(`/${RESET_HASH}`)}`);
      else setErr((e as Error).message);
    }
  };
  const cancel = async () => {
    setErr("");
    try {
      await api("/api/me/reset/cancel", {});
      await load();
    } catch (e) {
      setErr((e as Error).message);
    }
  };
  if (r.pending)
    return (
      <div class="control-aside" data-reset-pending={r.pending.mine ? "mine" : "other"}>
        <p>
          <b>Your devices reset</b> <Time at={r.pending.due} on />. Then this page reloads, and this browser becomes your first device. Your other devices
          and your email have been told, and any of them can cancel it.
        </p>
        {r.pending.mine ? (
          <button class="secondary" data-reset-cancel onClick={() => void cancel()}>
            Cancel the reset
          </button>
        ) : null}
        {err ? <p class="control-error">{err}</p> : null}
      </div>
    );
  return (
    <div class="control-aside" data-reset-offer>
      {!open ? (
        <button class="link" data-reset-open onClick={() => setOpen(true)}>
          Can't approve from another device, and no recovery code?
        </button>
      ) : (
        <>
          <p>
            <b>Reset your devices.</b> After a 48-hour wait, this browser becomes your account's first device. Before you start:
          </p>
          <ul>
            <li>Every device, passkey and recovery code you have stops working.</li>
            <li>Each machine is dropped, and you join it again on the machine.</li>
            <li>Teammates see that your key changed, and check it again.</li>
            <li>Anything end to end encrypted to your old devices can't be read.</li>
          </ul>
          <p class="dim">Your other devices and your email are told when it starts, and any of them can cancel it. It needs a GitHub sign-in from the last ten minutes.</p>
          <button class="secondary danger" data-reset-start onClick={() => void start()}>
            Reset my devices in 48 hours
          </button>
        </>
      )}
      {err ? <p class="control-error">{err}</p> : null}
    </div>
  );
}

export function ResetWarning({ s }: { s: ControlSession }) {
  const [r, load] = useReset(false);
  const [err, setErr] = useState("");
  if (!r?.pending) return null;
  const p = r.pending;
  const cancel = async () => {
    setErr("");
    try {
      const sig = await signText(s.keys, cancelBody(s.account, p.started_at));
      await api("/api/me/reset/cancel", { device: s.keys.id, sig });
      await load();
    } catch (e) {
      setErr((e as Error).message);
    }
  };
  return (
    <div class="control-dropped" role="alert" data-reset-warning>
      <p>
        <b>A reset of your devices is waiting.</b> It started <Time at={p.started_at} on /> from a GitHub sign-in, and happens <Time at={p.due} on />: every
        device stops working. If it wasn't you, cancel it.
      </p>
      <div class="control-dropped-actions">
        <button class="primary" data-reset-warning-cancel onClick={() => void cancel()}>
          This wasn't me: cancel
        </button>
      </div>
      {err ? <p class="control-error">{err}</p> : null}
    </div>
  );
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
// - `PasskeyResetWarning`, in *Devices and machines*: one is waiting, with
//   "This wasn't me: cancel", signed by this device's key.

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
          <b>Your passkeys reset</b> <Time at={r.pending.due} on />: every passkey is removed, and you add one again. Your devices and your email have been
          told, and any of your devices or passkeys can cancel it.
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
            <b>Reset your passkeys.</b> After a 48-hour wait, every passkey of yours is removed and you can add one again. Your devices, machines and recovery
            codes stay as they are.
          </p>
          <p class="dim">Your devices and your email are told when it starts, and any of your devices or passkeys can cancel it. It needs a GitHub sign-in from the last ten minutes.</p>
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
