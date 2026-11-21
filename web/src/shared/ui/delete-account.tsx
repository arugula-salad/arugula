// Deleting the account (#173): what goes with it, what stands in the way,
// and typing its login to confirm. Control's Sign-in and account dialog
// (shell/trust/account.tsx) and the app's Settings › You › Sign-in and
// security draw it, each under its own heading and sending
// `/api/me/delete` its own way; `deleted` is what follows (signing out).

import { useEffect, useState } from "preact/hooks";
import type { AccountCall, DeletePreview } from "../account";

export function DeleteAccountSteps({ call, back, deleted }: { call: AccountCall; back: () => void; deleted: () => void }) {
  const [p, setP] = useState<DeletePreview | null>(null);
  const [typed, setTyped] = useState("");
  const [err, setErr] = useState("");
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    call<DeletePreview>("GET", "/api/me/delete").then(setP, (e: Error) => setErr(e.message));
  }, []);
  const blocked = !p || p.blockers.length > 0 || p.pays.length > 0;
  const matches = !!p && typed.trim().toLowerCase() === p.confirm.trim().toLowerCase();
  return (
    <>
      {p ? (
        <>
          <p>This can't be undone. Arugula cloud forgets:</p>
          <ul class="control-steps" data-delete-what>
            <li>your account, your passkeys, its link to your GitHub account, and every session;</li>
            <li>
              your {p.machines} machine{p.machines === 1 ? "" : "s"}: they stop reaching Arugula cloud (Arugula keeps running on them, reachable only locally);
            </li>
            {p.disband.map((t) => (
              <li key={t.team} data-disband={t.team}>
                the team <b>{t.name}</b>, which you founded{t.others ? `: its ${t.others} other member${t.others === 1 ? " loses" : "s lose"} it, and its machines go back to their owners` : ""};
              </li>
            ))}
            <li>your push subscriptions, usage counts and team requests.</li>
          </ul>
          {p.stays_in.length ? (
            <p class="dim">
              You stay listed in {p.stays_in.join(", ")} until an owner removes you; with no devices behind your name, it lets nothing in. Its owners are told.
            </p>
          ) : null}
          {p.blockers.length || p.pays.length ? (
            <div class="control-error" data-delete-blocked>
              <p>First:</p>
              <ul>
                {p.blockers.map((b) => (
                  <li key={b}>{b}.</li>
                ))}
                {p.pays.map((b) => (
                  <li key={b}>Cancel {b}.</li>
                ))}
              </ul>
            </div>
          ) : (
            <label class="control-confirm">
              Type <b>{p.confirm}</b> to confirm:
              <input
                type="text"
                data-delete-confirm
                autocomplete="off"
                spellcheck={false}
                value={typed}
                onInput={(e) => setTyped((e.target as HTMLInputElement).value)}
              />
            </label>
          )}
        </>
      ) : null}
      {err ? <p class="control-error">{err}</p> : null}
      <div class="prompt-buttons">
        <button class="secondary" onClick={back}>Back</button>
        <button
          class="primary danger"
          data-delete-go
          disabled={blocked || !matches || busy}
          onClick={() => {
            setBusy(true);
            setErr("");
            call("POST", "/api/me/delete", { confirm: typed }).then(deleted, (e: Error) => {
              setBusy(false);
              setErr(e.message);
            });
          }}
        >
          Delete my account
        </button>
      </div>
    </>
  );
}
