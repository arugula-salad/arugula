// A deploy's approval (M218), on control's page, which holds this
// browser's device keys ("The platform page is on app.arugula.io, not on
// the keys' origin"). The platform page's Approve and Deny come here as
// `#deploy=dep_…`; so does a tap on the push, through the platform page.
//
// The page reads the deploy, hashes the operation's bytes itself and shows
// them, and hashes its `planned` event again, so where it goes, what it
// deploys and who planned it all come from hashed bytes. Then it asks for a
// passkey over the approval's bytes (challenge: their SHA-256) and signs
// S37's approval with this device's key. Both go to the platform: it checks
// the device's signature against the account's devices (an owner's
// browser, certified by browsers back to the root), and has the hub check
// the passkey's assertion (registered to this account, user verified, over
// this approval). Without a passkey nothing is approved, so whatever drives
// this browser can't approve alone. When this browser's keys came from that
// passkey (#764), the signing key is derived from it again too.

import { useEffect, useState } from "preact/hooks";
import { Modal } from "../../shared/ui/modal";
import { api, type ControlSession } from "../control";
import {
  DEFAULT_LIFE_MS,
  actorOf,
  approvalBody,
  checkPlan,
  eventCanonical,
  parseOperation,
  sha256Hex,
  signApproval,
  type CheckedPlan,
  type Decision,
  type DeployEvent,
  type Operation,
} from "../e2e/deploy.ts";
import { PRF_SALTS, prfBytes, prfSignKey } from "../e2e/keys.ts";
import { platformHref } from "../platform-links";
import { clearHash } from "./common";

export const DEPLOY_HASH = /^#deploy=(dep_[0-9a-f]{32})$/;

interface Deploy {
  id: string;
  project: { id: string; name: string };
  environment: { id: string; name: string; driver: string };
  source: { kind: string; ref: string; where: string };
  plan: string;
  /** The plan's text (M220): what its digest is over. */
  plan_text: string | null;
  state: string;
  planned_by: string;
  planned_by_name: string;
  agent: { machine: string; machine_name: string } | null;
  operation: { body: string; hash: string };
  planned_event: DeployEvent | null;
  stale: { deploy: string; seq: number } | null;
  may_approve: boolean;
}

/** What the page shows, read from hashed bytes: the operation, and who
 * planned it from its `planned` event. */
interface Checked {
  op: Operation;
  planner: string;
  /** The plan's text, which hashes to the operation's `plan` (M220). */
  plan: CheckedPlan;
}

/** The deploy's operation and its `planned` event check out: the bytes hash
 * to what the platform says and say what the deploy says, and the event
 * hashes to the one the operation names. What to show, or why not. */
async function check(d: Deploy): Promise<Checked | string> {
  const op = parseOperation(d.operation.body);
  if (!op || (await sha256Hex(d.operation.body)) !== d.operation.hash) return "its operation's bytes don't match their hash";
  if (
    op.project !== d.project.id ||
    op.environment !== d.environment.id ||
    op.name !== d.environment.name ||
    op.plan !== d.plan ||
    op.source.kind !== d.source.kind ||
    op.source.ref !== d.source.ref ||
    op.source.where !== d.source.where
  ) {
    return "its operation says something other than the deploy";
  }
  const e = d.planned_event;
  if (!e || e.kind !== "planned" || e.deploy !== d.id || e.environment !== op.environment) return "its plan isn't in the log";
  if ((await sha256Hex(eventCanonical(e))) !== e.hash || e.hash !== op.planned) return "its plan's event doesn't match the operation";
  let detail: { plan?: string; source?: { kind?: string; ref?: string; where?: string } };
  try {
    detail = JSON.parse(e.detail);
  } catch {
    return "its plan's event can't be read";
  }
  if (detail.plan !== op.plan || detail.source?.kind !== op.source.kind || detail.source?.ref !== op.source.ref || detail.source?.where !== op.source.where) {
    return "its plan's event says another plan";
  }
  const who = actorOf(e.actor);
  if (!who || who.account !== d.planned_by || (who.machine ?? null) !== (d.agent?.machine ?? null)) return "who planned it doesn't match its log";
  const name = d.planned_by_name || "someone";
  // What the driver will do, as the operation's plan digest binds it: the
  // text must hash to it, or it isn't what's being signed (M220).
  const plan = await checkPlan(d.plan_text, op.plan);
  if (typeof plan === "string") return plan;
  if (plan.driver !== d.environment.driver) return "its plan names another driver than its environment's";
  return { op, plan, planner: who.machine ? `${name}'s agent on ${d.agent?.machine_name || who.machine}` : name };
}

const unb64u = (s: string) => Uint8Array.from(atob(s.replace(/-/g, "+").replace(/_/g, "/")), (c) => c.charCodeAt(0));
const b64u = (b: ArrayBuffer) =>
  btoa(String.fromCharCode(...new Uint8Array(b)))
    .replace(/\+/g, "-")
    .replace(/\//g, "_")
    .replace(/=+$/, "");

/** What the platform takes of a passkey's assertion (the hub checks it). */
interface Assertion {
  id: string;
  clientDataJSON: string;
  authenticatorData: string;
  signature: string;
}

/** Ask for a passkey (user verification required), its challenge SHA-256
 * of the approval's own bytes: its assertion, and the PRF output for
 * signing when it gives one. */
async function passkey(body: string): Promise<{ assertion: Assertion; second?: ArrayBuffer }> {
  const challenge = new Uint8Array(await crypto.subtle.digest("SHA-256", new TextEncoder().encode(body)));
  // Only the account's own passkeys: a password manager may keep others
  // for this site (one control refused to keep, or another account's).
  const mine = await api<{ passkeys: { id: string }[] }>("/api/me/passkeys").then(
    (r) => r.passkeys.map((p) => ({ type: "public-key" as const, id: unb64u(p.id) })),
    () => [],
  );
  const cred = (await navigator.credentials.get({
    publicKey: {
      challenge,
      ...(mine.length ? { allowCredentials: mine } : {}),
      // Control's passkeys are made for the host its page is on (its
      // `rp_id`: the host, no port), which is this page's: nothing to ask,
      // and no sign-in challenge issued for nothing.
      rpId: location.hostname,
      userVerification: "required",
      timeout: 5 * 60 * 1000,
      extensions: { prf: { eval: { first: PRF_SALTS.noise, second: PRF_SALTS.sign } } } as AuthenticationExtensionsClientInputs,
    },
  })) as PublicKeyCredential | null;
  if (!cred) throw new Error("no passkey chosen");
  const r = cred.response as AuthenticatorAssertionResponse;
  return {
    assertion: { id: b64u(cred.rawId), clientDataJSON: b64u(r.clientDataJSON), authenticatorData: b64u(r.authenticatorData), signature: b64u(r.signature) },
    second: prfBytes((cred.getClientExtensionResults() as { prf?: { results?: { second?: unknown } } }).prf?.results?.second),
  };
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
      const a = { deploy: d.id, op: d.operation.hash, account: s.account, decision, expires: Date.now() + DEFAULT_LIFE_MS, by: s.keys.id };
      const { assertion, second } = await passkey(approvalBody(a));
      let key = s.keys.sign.privateKey;
      // This browser's key is the passkey's own (#764) only if it derives
      // to this device's: otherwise, or if it won't derive, its own.
      if (second) {
        const derived = await prfSignKey(second).catch((e: Error) => {
          console.warn("arugula: the passkey's PRF output gave no key:", e);
          return null;
        });
        if (derived?.pub === s.keys.signPub) key = derived.key;
      }
      const signed = await signApproval(a, key);
      const after = await api<Deploy>(`/api/deploys/${d.id}/${decision === "once" ? "approve" : "deny"}`, {
        op: signed.op,
        expires: signed.expires,
        by: signed.by,
        sig: signed.sig,
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
              <b>{d.project.name}</b>: {ok.op.source.kind} <code>{ok.op.source.ref.slice(0, 12)}</code> from <code>{ok.op.source.where}</code>, planned
              by {ok.planner}.
            </p>
          ) : null}
          {ok?.plan.rollBack ? (
            <p class="control-error" data-deploy-roll-back={ok.plan.rollBack.deploy}>
              This is a roll back to <code>{ok.plan.rollBack.deploy}</code>
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
          <p class="dim">The operation you sign:</p>
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
            <p data-deploy-state={d.state}>
              {d.stale ? "It's stale: another deploy was applied to the environment after it was planned." : `It's ${d.state}, not waiting for you.`}
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
