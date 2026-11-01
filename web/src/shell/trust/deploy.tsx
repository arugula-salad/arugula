// A deploy's approval (M218), on control's page, where the account's
// passkeys are made (their RP is control's host). Since DK5 (#386) the
// platform page approves in place, and comes here (`#deploy=dep_…`) only
// where its browser can't use control's passkeys (related origins: the
// 1Password extension, Firefox before 152; the Linux app's window, which has
// no WebAuthn, opens it in the browser).
//
// The page reads the deploy, hashes the operation's bytes itself and shows
// them, and hashes its `planned` event again, so where it goes, what it
// deploys and who planned it all come from hashed bytes. Then it asks for a
// passkey, user verified, over the approval's bytes (v2, DK4: challenge
// their SHA-256), and sends the assertion: that is the approval's
// signature. No device key signs it (aru-126). The platform checks the
// assertion against the account's published passkey log and that the
// account owns the project now. Whatever drives this browser can't make the
// assertion without the person. The checks and the ceremony are shared
// with the platform page (../../shared/deploy-approval.ts).

import { useEffect, useState } from "preact/hooks";
import { deployStateText, deployTitle, shortRef, staleNote } from "../../shared/deploys";
import { DeployAsked } from "../../shared/ui/deploy-asked";
import { deployTone } from "../../shared/tones";
import { Modal } from "../../shared/ui/modal";
import { StateChip } from "../../shared/ui/state-chip";
import { api, type ControlSession } from "../control";
import {
  DEFAULT_LIFE_MS,
  checkDeploy,
  passkeyOver,
  type Approval,
  type Assertion,
  type Checked,
  type Decision,
  type DeployToCheck,
} from "../../shared/deploy-approval.ts";
import { platformHref } from "../platform-links";
import { clearHash } from "./common";

export const DEPLOY_HASH = /^#deploy=(dep_[0-9a-f]{32})$/;

interface Deploy extends DeployToCheck {
  /** The platform's reading of the commit (#333): not in the signed
   * operation, so only ever shown as the platform's. */
  subject?: string | null;
  expected_version?: string | null;
  requested_at?: number | null;
  state: string;
  stale: { deploy: string; seq: number; why?: string } | null;
  may_approve: boolean;
}

const check = checkDeploy;

/** Ask for one of the account's own passkeys (a password manager may keep
 * others for this site: one control refused to keep, or another account's)
 * on control's own host, which is their RP: its assertion over `a`. */
async function passkey(a: Approval): Promise<Assertion> {
  const mine = await api<{ passkeys: { id: string }[] }>("/api/me/passkeys").then(
    (r) => r.passkeys.map((p) => p.id),
    () => [],
  );
  return passkeyOver(a, location.hostname, mine);
}

export function DeployPrompt({ s, id }: { s: ControlSession; id: string }) {
  const [d, setD] = useState<Deploy | null>(null);
  const [shown, setShown] = useState<Checked | string | null>(null);
  const [err, setErr] = useState("");
  const [busy, setBusy] = useState(false);
  const [done, setDone] = useState<string | null>(null);
  useEffect(() => {
    api<Deploy>(`/api/deploys/${id}`).then(
      async (d) => {
        setShown(await check(d));
        setD(d);
      },
      (e: Error) => setErr(e.message),
    );
  }, [id]);
  const back = platformHref(`/deploys/${id}`);
  const bad = typeof shown === "string" ? shown : null;
  const ok = shown && typeof shown !== "string" ? shown : null;
  const decide = async (decision: Decision) => {
    if (!d) return;
    setBusy(true);
    setErr("");
    try {
      const a: Approval = { v: 2, deploy: d.id, op: d.operation.hash, account: s.account, decision, expires: Date.now() + DEFAULT_LIFE_MS };
      const assertion = await passkey(a);
      const after = await api<Deploy>(`/api/deploys/${d.id}/${decision === "once" ? "approve" : "deny"}`, {
        op: a.op,
        expires: a.expires,
        passkey: assertion,
      });
      setD(after);
      setDone(decision === "once" ? "Approved. Its driver run can apply it now." : "Denied. It won't be applied.");
    } catch (e) {
      setErr((e as Error).message);
    }
    setBusy(false);
  };
  return (
    <Modal close={clearHash}>
      <h2>{ok ? `Deploy to ${ok.op.name}?` : "A deploy"}</h2>
      {err ? <p class="control-error" data-deploy-error>{err}</p> : null}
      {d ? (
        <div class="deploy-approve" data-deploy-approve={d.id}>
          {ok ? (
            <p>
              <b>{d.project.name}</b>: {ok.op.source.kind} <code title={ok.op.source.ref}>{shortRef(ok.op.source.ref)}</code> from <code>{ok.op.source.where}</code>, planned
              by {ok.planner}.
            </p>
          ) : null}
          {ok && (d.subject || d.expected_version) ? (
            // Not signed: beside the checked facts, never in place of them (#333).
            <p class="dim" data-deploy-reading>
              The platform reads this as: {deployTitle(d)}
            </p>
          ) : null}
          {ok && d.requested_at ? (
            <p class="dim deploy-when">
              <DeployAsked at={d.requested_at} waiting={d.state === "pending"} />
            </p>
          ) : null}
          {ok?.plan.rollBack ? (
            <p class="control-error" data-deploy-roll-back={ok.plan.rollBack.deploy}>
              This is a rollback to <code>{ok.plan.rollBack.deploy}</code>
              {ok.plan.rollBack.image ? (
                <>
                  , running the image <code>{ok.plan.rollBack.image}</code> again
                </>
              ) : null}
              .
            </p>
          ) : null}
          {ok ? (
            <>
              <p class="dim">What its driver will do (the plan you sign):</p>
              <pre class="deploy-op" data-deploy-plan>
                {ok.plan.text}
              </pre>
            </>
          ) : null}
          <p class="dim">The operation your passkey confirms:</p>
          <pre class="deploy-op" data-deploy-op>
            {d.operation.body}
          </pre>
          <p class="dim deploy-hash">
            Its hash <code data-deploy-op-hash>{d.operation.hash}</code>
          </p>
          {bad ? (
            <p class="control-error" data-deploy-mismatch>
              Don't approve this: {bad}.
            </p>
          ) : done ? (
            <p data-deploy-done>{done}</p>
          ) : !d.may_approve ? (
            // Its state as the platform page shows it (UI audit #183).
            <p data-deploy-state={d.state}>
              <StateChip tone={deployTone(d.state)}>{deployStateText(d.state)}</StateChip>{" "}
              {d.stale ? (
                <>
                  {staleNote(d.stale, d.environment.name)} <a href={platformHref(`/deploys/${d.stale.deploy}`)}>{d.stale.deploy.slice(0, 12)}</a>
                </>
              ) : (
                "It isn't waiting for your approval."
              )}
            </p>
          ) : s.passkeys === 0 ? (
            <p data-deploy-no-passkey>
              Deploys are approved and denied with a passkey, and your account has none. Add one first (the account menu, Sign-in and account).
            </p>
          ) : null}
          <div class="prompt-buttons">
            <button class="secondary" data-deploy-back onClick={() => (back ? location.assign(back) : clearHash())}>
              {back ? "Back" : "Close"}
            </button>
            {d.may_approve && !bad && !done && s.passkeys > 0 ? (
              <>
                <button class="secondary" data-deploy-deny disabled={busy} onClick={() => void decide("deny")}>
                  Deny
                </button>
                <button class="primary" data-deploy-approve-button disabled={busy} onClick={() => void decide("once")}>
                  Approve with passkey
                </button>
              </>
            ) : null}
          </div>
        </div>
      ) : err ? null : (
        <p class="dim">Looking it up…</p>
      )}
    </Modal>
  );
}
