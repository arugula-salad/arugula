// The account's recovery codes (M17): the form that uses one on a browser
// no device can approve, the codes shown once to keep, and the count left
// with Make new codes, in Sign-in and account, under the account's name
// (#102).

import { useState } from "preact/hooks";
import { CopyButton, CopyText, download } from "../../shared/ui/copy";
import { Modal } from "../../shared/ui/modal";
import type { ControlSession } from "../control";
import { PasskeyNudge } from "./devices";

/** What other people see (#102), changed in place: Sign-in and account's
 * own row under its title (UI audit #174: it sat under Recovery codes). */
export function NameLine({ s }: { s: ControlSession }) {
  const [editing, setEditing] = useState(false);
  const [name, setName] = useState(s.name);
  const [err, setErr] = useState("");
  if (!editing)
    return (
      <p class="dim">
        Name <b data-account-name>{s.name || "none yet"}</b>{" "}
        <button
          class="link"
          data-edit-name
          onClick={() => {
            setName(s.name);
            setEditing(true);
          }}
        >
          Change
        </button>
      </p>
    );
  return (
    <form
      class="control-code control-name"
      onSubmit={(e) => {
        e.preventDefault();
        s.setName(name).then(
          () => setEditing(false),
          (x: Error) => setErr(x.message),
        );
      }}
    >
      <input data-name-input maxLength={64} value={name} onInput={(e) => setName((e.target as HTMLInputElement).value)} aria-label="Name" autoFocus />
      <button type="submit" class="primary" data-save-name disabled={!name.trim()}>
        Save
      </button>
      {err ? <p class="control-error">{err}</p> : null}
    </form>
  );
}

export function RecoveryForm({ s, label = "Lost your other devices? Use a recovery code", open: startOpen = false }: { s: ControlSession; label?: string; open?: boolean }) {
  const [open, setOpen] = useState(startOpen);
  const [code, setCode] = useState("");
  const [err, setErr] = useState("");
  // A button that looks like one (#327): as a link it read as a sentence.
  if (!open)
    return (
      <button class="secondary" data-use-recovery onClick={() => setOpen(true)}>
        {label}
      </button>
    );
  return (
    <form
      class="control-code"
      onSubmit={(e) => {
        e.preventDefault();
        s.useRecoveryCode(code).catch((x: Error) => setErr(x.message));
      }}
    >
      <input placeholder="Recovery code" value={code} onInput={(e) => setCode((e.target as HTMLInputElement).value)} aria-label="Recovery code" data-recovery-input autoFocus={startOpen} />
      <button type="submit" class="primary">
        Use it
      </button>
      {err ? <p class="control-error">{err}</p> : null}
    </form>
  );
}

/** Once, after the account's first device (or new codes): the codes to keep. */
export function RecoveryCodes({ s }: { s: ControlSession }) {
  const all = s.recoveryCodes!.join("\n") + "\n";
  const [stored, setStored] = useState(false);
  return (
    <Modal>
      <h2>Your recovery codes</h2>
      <p>If you lose every device that can approve new ones, one of these lets a new browser in. Each works once. Keep them somewhere safe and offline: they're shown only now, and this service never had them.</p>
      {s.recoveryCodes!.map((c) => (
        <p key={c}>
          <CopyText text={c} data-recovery-code />
        </p>
      ))}
      <div class="prompt-buttons">
        <CopyButton text={all} label="Copy both" />
        <button class="secondary" data-download-codes onClick={() => download("arugula-recovery-codes.txt", all)}>
          Download .txt
        </button>
      </div>
      <label class="control-check">
        <input type="checkbox" data-stored-codes checked={stored} onChange={(e) => setStored((e.target as HTMLInputElement).checked)} />
        I've stored these somewhere safe
      </label>
      <PasskeyNudge s={s} />
      <div class="prompt-buttons">
        <button class="primary" data-saved-codes disabled={!stored} onClick={() => s.savedRecoveryCodes()}>
          Continue
        </button>
      </div>
    </Modal>
  );
}

/** The account's recovery codes: Sign-in and account's, with the passkeys
 * (they were at the foot of Devices and machines). */
export function RecoveryLeft({ s }: { s: ControlSession }) {
  const [renewing, setRenewing] = useState(false);
  const [err, setErr] = useState("");
  const left = s.recoveryLeft;
  return (
    <>
      <h3 class="control-group">Recovery codes</h3>
      <p class="dim">
        <span data-recovery-left={left}>
          {left === 0 ? "No recovery codes left." : `${left} recovery code${left === 1 ? "" : "s"} left.`}
        </span>{" "}
        {renewing ? "New codes replace these: the old ones stop working. " : ""}
        {s.canApprove("recovery") ? null : <span data-codes-elsewhere>Make new ones from a browser or phone: the desktop app doesn't. </span>}
        <button
          hidden={!s.canApprove("recovery")}
          class={renewing ? "link danger" : "link"}
          data-new-codes
          onClick={() => {
            if (!renewing) return setRenewing(true);
            setRenewing(false);
            s.newRecoveryCodes().catch((e: Error) => setErr(e.message));
          }}
        >
          {renewing ? "Make new ones" : "Make new codes"}
        </button>
      </p>
      {err ? <p class="control-error">{err}</p> : null}
    </>
  );
}
