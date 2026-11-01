// Deploys waiting for an owner (M218): a card per deploy, showing where it
// goes, what it deploys, the plan's digest and the operation an approval
// signs. `/deploys/:id` is one deploy with its approvals and log; the
// inbox lists the deploys waiting for the reader, and a project's
// environments their history (`DeployRow`).
//
// Approving takes a passkey over the approval's bytes (DK4), and since DK5
// (#386) this page asks for it in place: a push opens the deploy here, and
// its card approves or denies it on one screen (`DeployDecide`). The page
// first checks the operation's bytes against their hash and its planned
// event, as control's dialog does (../../shared/deploy-approval.ts), and
// asks the platform whether the reader may decide it now
// (`/api/deploys/{id}/can-decide`), so it shows why not instead of a button
// that would fail. The passkey is control's (its RP is control's host,
// aru-078): this page uses it as a related origin, which control's
// `/.well-known/webauthn` lists. Where that can't work (the 1Password
// extension, Firefox before 152, the Linux app's window with no WebAuthn),
// it offers control's `#deploy=` dialog instead, and says why.

import { useEffect, useState } from "preact/hooks";
import {
  DEFAULT_LIFE_MS,
  NotHere,
  checkDeploy,
  passkeyOver,
  passkeysNotHere,
  type Approval,
  type Checked,
  type Decision,
  type DeployEvent,
} from "../../shared/deploy-approval";
import { desktopPlatform, openOutside } from "../../shared/desktop";
import { platformFetch, refusalOf } from "../../shared/maintenance";
import { deployEventState, deployStateText, deployTitle, plannerText, staleNote } from "../../shared/deploys";
import { DeployAsked } from "../../shared/ui/deploy-asked";
import { CopyButton } from "../../shared/ui/copy";
import { PageHeader } from "../../shared/ui/page-header";
import { StateChip } from "../../shared/ui/state-chip";
import { Time } from "../../shared/ui/time";
import { controlOrigin, session } from "../session";
import { names } from "../links";
import { follow, followRow, pathOf } from "../router";
import { lastMoved, shortRef, type DeploySummary } from "../deploys-text";
import { deployTone, toneClass } from "../swarm";
import { where } from "../where";
import { noteWaitingDeploys } from "../inbox-count";

export interface Deploy {
  id: string;
  project: { id: string; name: string };
  environment: { id: string; name: string; driver: string };
  source: { kind: string; ref: string; where: string };
  plan: string;
  /** The plan's text (M220): what an owner reads before they approve.
   * Null when the platform can't make it again. */
  plan_text: string | null;
  /** A rollback's deploy and what it goes back to, or null. */
  roll_back: { deploy: string } | null;
  state: string;
  planned_by: string;
  planned_by_name: string;
  agent: { machine: string; machine_name: string } | null;
  /** What the platform read of the deploy when it planned it (#333), or
   * null: the commit's first line, and a release's expected version. */
  subject?: string | null;
  expected_version?: string | null;
  created_at: number;
  /** When approval was last asked for, or null. */
  requested_at?: number | null;
  operation: { body: string; hash: string };
  /** Its `planned` event as stored, which the page hashes again. */
  planned_event?: DeployEvent | null;
  stale: { deploy: string; seq: number; why?: string } | null;
  may_approve: boolean;
  approvals: {
    id: string;
    /** 2: a passkey's assertion alone (DK4); 1: a device's signature too. */
    v?: number;
    decision: "once" | "deny";
    name: string;
    account: string;
    device_name: string;
    at: number;
    expires: number;
    passkey: { id: string } | null;
  }[];
  /** The run that applied it, once it started. */
  run?: string | null;
  /** `detail` is what the event said (a failure's reason, its run); `run`
   * the run it's by, if one. */
  events: { seq: number; kind: string; actor: string; by: string; at: number; hash: string; detail?: Record<string, unknown>; run?: string | null }[];
  log: { verified?: number; broken_at?: number };
}

async function get<T>(path: string): Promise<T> {
  const r = await platformFetch(path, { credentials: "same-origin" });
  const v = (await r.json().catch(() => ({}))) as T & { error?: string; reason?: string };
  if (!r.ok) throw new Error(refusalOf(r.status, v));
  return v;
}

/** Who planned it: a person, "<owner>'s agent on <machine>", or an agent
 * token's (../../shared/deploys). */
export const planner = (d: Pick<Deploy, "planned_by_name" | "agent">) => plannerText(d.planned_by_name || "a deleted account", d.agent);

/** A deploy's state, as people read it: control's prompt says it in the
 * same words (UI audit #183). */
export const stateText = deployStateText;

/** A deploy's state as its chip, wherever it shows (UI audit #183). */
export const DeployState = ({ state }: { state: string }) => (
  <StateChip tone={deployTone(state)} class="deploy-state" data-deploy-state-chip={state}>
    {stateText(state)}
  </StateChip>
);

async function post<T>(path: string, body: unknown): Promise<T> {
  const r = await platformFetch(path, {
    method: "POST",
    credentials: "same-origin",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(body),
  });
  const v = (await r.json().catch(() => ({}))) as T & { error?: string; reason?: string };
  if (!r.ok) throw new Error(refusalOf(r.status, v));
  return v;
}

/** The way round: the deploy on control's page (`#deploy=`), whose dialog
 * asks for the passkey on control's own host. */
export const approveHref = (id: string) => `${controlOrigin()}/#deploy=${id}`;

/** Control's dialog, from a click: in the person's browser from the Linux
 * app's window, which has no WebAuthn on any page (S46 q3); a plain link
 * elsewhere. */
function toControl(e: MouseEvent, href: string) {
  if (desktopPlatform() === "linux" && openOutside(href)) e.preventDefault();
}

/** What `GET /api/deploys/{id}/can-decide` answers (DK5). */
export interface CanDecide {
  can: boolean;
  /** Why not, as a code: not_pending, stale, superseded, expired,
   * not_an_owner, no_passkey. */
  reason: string | null;
  why: string | null;
  do: string | null;
  /** The account's live passkeys' credential ids, when it can. */
  passkeys: string[];
  /** Their RP ID: control's host. */
  rp: string | null;
  /** A superseded one's newer deploy. */
  newer?: string | null;
}

type Phase =
  | { kind: "checking" }
  | { kind: "bad"; why: string }
  | { kind: "cannot"; cd: CanDecide }
  | { kind: "elsewhere"; why: string }
  | { kind: "ready"; ok: Checked; cd: CanDecide; rp: string; deploy: string; hash: string };

/** A ready phase is for exactly this deploy and these bytes: what was
 * checked is what would be signed (the router reuses the page for another
 * deploy, and a ready phase must never outlive the one it checked). */
export const readyFor = (p: { kind: string; deploy?: string; hash?: string }, d: { id: string; operation: { hash: string } }) =>
  p.kind === "ready" && p.deploy === d.id && p.hash === d.operation.hash;

/** A deploy's title from what the operation signs alone (its environment):
 * where Approve is offered, the commit's subject and a release's expected
 * version, which the platform read and nobody signed, show labelled below
 * instead (`data-deploy-reading`). */
export const checkedTitle = (d: Deploy) => `Deploy ${d.source.ref.slice(0, 8)} to ${d.environment.name}`;

/** The title a deploy's card and page show: the checked one while it waits
 * for approval. */
export const shownTitle = (d: Deploy) => (d.state === "pending" ? checkedTitle(d) : deployTitle(d));

/** A pending deploy's Approve and Deny, on this page (DK5): the operation
 * checked against its hash and shown, then whether the reader may decide
 * it, then the passkey. Control's dialog where this page can't use
 * control's passkeys. */
export function DeployDecide({ d, onDecided }: { d: Deploy; onDecided: (after: Deploy, decision: Decision) => void }) {
  const [phase, setPhase] = useState<Phase>({ kind: "checking" });
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState("");
  useEffect(() => {
    let gone = false;
    // Another deploy, or other bytes: nothing checked before stands.
    setPhase({ kind: "checking" });
    setErr("");
    const set = (p: Phase) => void (gone || setPhase(p));
    const deploy = d.id;
    const hash = d.operation.hash;
    void (async () => {
      const ok = await checkDeploy(d);
      if (typeof ok === "string") return set({ kind: "bad", why: ok });
      let cd: CanDecide;
      try {
        cd = await get<CanDecide>(`/api/deploys/${d.id}/can-decide`);
      } catch (e) {
        return set({ kind: "cannot", cd: { can: false, reason: "error", why: (e as Error).message, do: null, passkeys: [], rp: null } });
      }
      if (!cd.can) return set({ kind: "cannot", cd });
      const rp = cd.rp || new URL(controlOrigin()).hostname;
      const not = await passkeysNotHere(rp);
      set(not ? { kind: "elsewhere", why: not } : { kind: "ready", ok, cd, rp, deploy, hash });
    })();
    return () => void (gone = true);
  }, [d.id, d.operation.hash]);
  const decide = async (decision: Decision) => {
    // Only what was checked and shown: this deploy, these bytes.
    if (phase.kind !== "ready" || !readyFor(phase, d)) return;
    setBusy(true);
    setErr("");
    try {
      const account = session.me?.account;
      if (!account) throw new Error("sign in first");
      const a: Approval = { v: 2, deploy: phase.deploy, op: phase.hash, account, decision, expires: Date.now() + DEFAULT_LIFE_MS };
      const passkey = await passkeyOver(a, phase.rp, phase.cd.passkeys);
      const after = await post<Deploy>(`/api/deploys/${phase.deploy}/${decision === "once" ? "approve" : "deny"}`, { op: a.op, expires: a.expires, passkey });
      onDecided(after, decision);
    } catch (e) {
      if (e instanceof NotHere) setPhase({ kind: "elsewhere", why: e.message });
      else setErr((e as Error).message);
    }
    setBusy(false);
  };
  const href = approveHref(d.id);
  // The plan and the operation folded, where Approve isn't offered here;
  // open beside it, as checked.
  const folded = (
    <>
      {d.plan_text ? (
        <details class="deploy-op" data-deploy-plan-text>
          <summary>{d.roll_back ? "The plan: a rollback" : "The plan"}</summary>
          <pre>{d.plan_text}</pre>
        </details>
      ) : null}
      <details class="deploy-op">
      <summary>The operation an approval signs</summary>
      <pre data-deploy-op>{d.operation.body}</pre>
      <p class="dim">
        Its hash <code class="deploy-digest">{d.operation.hash}</code>
      </p>
    </details>
    </>
  );
  // A ready phase another deploy's (for the moment before the effect runs)
  // reads as checking.
  const shown: Phase = phase.kind === "ready" && !readyFor(phase, d) ? { kind: "checking" } : phase;
  switch (shown.kind) {
    case "checking":
      return (
        <>
          {folded}
          <p class="dim deploy-why" data-deploy-decide="checking">
            Checking it before your passkey…
          </p>
        </>
      );
    case "bad":
      return (
        <>
          {folded}
          <p class="control-error" data-deploy-decide="bad" data-deploy-mismatch>
            Don't approve this: {shown.why}.
          </p>
        </>
      );
    case "cannot":
      return (
        <>
          {folded}
          <p class="deploy-note" data-deploy-decide="cannot" data-deploy-cannot={shown.cd.reason ?? ""}>
            {shown.cd.why} {shown.cd.do}
            {shown.cd.newer ? (
              <>
                {" "}
                <a href={`/deploys/${shown.cd.newer}`} onClick={(e) => follow(e, `/deploys/${shown.cd.newer}`)} data-deploy-newer>
                  {shown.cd.newer.slice(0, 12)}
                </a>
              </>
            ) : null}
          </p>
        </>
      );
    case "elsewhere": {
      const app = desktopPlatform() === "linux";
      return (
        <>
          {folded}
          <div class="deploy-actions" data-deploy-decide="elsewhere">
            <a class="primary" href={href} onClick={(e) => toControl(e, href)} data-deploy-on-control>
              Approve on Control
            </a>
            <p class="dim deploy-why" data-deploy-why-elsewhere>
              {shown.why.charAt(0).toUpperCase() + shown.why.slice(1)}.{" "}
              {app ? "This opens Control's page in your browser, where your passkey approves or denies it." : "Control's page asks for your passkey, and approves or denies it there."}
            </p>
          </div>
        </>
      );
    }
    case "ready": {
      const { ok } = shown;
      return (
        <div class="deploy-decide" data-deploy-decide="ready">
          <p>
            <b>{d.project.name}</b>: {ok.op.source.kind} <code title={ok.op.source.ref}>{shortRef(ok.op.source.ref)}</code> to <b>{ok.op.name}</b> from{" "}
            <code>{ok.op.source.where}</code>, planned by {ok.planner}.
          </p>
          {d.subject || d.expected_version ? (
            // Not signed: beside the checked facts, never in place of them (#333).
            <p class="dim" data-deploy-reading>
              The platform reads this as: {deployTitle(d)}
            </p>
          ) : null}
          {ok.plan.rollBack ? (
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
          <div class="deploy-op">
            <p class="dim">What its driver will do (the plan you sign):</p>
            <pre data-deploy-plan-signed>{ok.plan.text}</pre>
            <p class="dim">The operation your passkey confirms:</p>
            <pre data-deploy-op-signed>{d.operation.body}</pre>
            <p class="dim">
              Its hash <code class="deploy-digest" data-deploy-op-hash>{d.operation.hash}</code>
            </p>
          </div>
          {err ? (
            <p class="control-error" data-deploy-error>
              {err}
            </p>
          ) : null}
          <div class="deploy-actions">
            <button class="secondary" data-deploy-deny disabled={busy} onClick={() => void decide("deny")}>
              Deny with passkey
            </button>
            <button class="primary" data-deploy-approve disabled={busy} onClick={() => void decide("once")}>
              Approve with passkey
            </button>
            <p class="dim deploy-why">
              Or{" "}
              <a href={href} onClick={(e) => toControl(e, href)} data-deploy-on-control>
                approve on Control
              </a>
              , if your passkey isn't offered here.
            </p>
          </div>
        </div>
      );
    }
  }
}

/** One deploy's card. `link`: its heading opens the deploy's own page.
 * The plan's text is open on the deploy's own page, or with `planOpen`.
 * On the deploy's own page its heading and state are the page's
 * (`headless`); elsewhere it's a card in the swarm's language, its bar
 * its state's tone (UI audit #167). A pending one is approved or denied
 * on it (DK5); `onDecided` hears the deploy after. */
export function DeployCard({
  d: given,
  link,
  planOpen,
  headless,
  onDecided,
}: {
  d: Deploy;
  link?: boolean;
  planOpen?: boolean;
  headless?: boolean;
  onDecided?: (after: Deploy) => void;
}) {
  const [after, setAfter] = useState<{ d: Deploy; said: string } | null>(null);
  const d = after && after.d.id === given.id ? after.d : given;
  const decided = (x: Deploy, decision: Decision) => {
    setAfter({ d: x, said: decision === "once" ? "Approved. Its driver run can apply it now." : "Denied. It won't be applied." });
    onDecided?.(x);
  };
  // A pending one's plan and operation are `DeployDecide`'s: open beside
  // Approve, as checked; folded otherwise.
  const deciding = d.state === "pending";
  const path = `/deploys/${d.id}`;
  const project = pathOf({ page: "project", project: d.project.id }, names);
  return (
    <article class={headless ? "deploy-card" : `deploy-card sw-card ${toneClass(deployTone(d.state))}`} data-deploy-card={d.id} data-deploy-state={d.state}>
      {!headless && (
        <header class="deploy-card-head">
          <h2>
            {link ? (
              <a href={path} onClick={(e) => follow(e, path)}>
                {shownTitle(d)}
              </a>
            ) : (
              <>{shownTitle(d)}</>
            )}
          </h2>
          <DeployState state={d.state} />
        </header>
      )}
      <p class="dim deploy-when">
        <DeployAsked at={d.requested_at} waiting={d.state === "pending"} />
      </p>
      <dl class="deploy-facts">
        <dt>Project</dt>
        <dd>
          <a href={project} onClick={(e) => follow(e, project)} data-deploy-project>
            {d.project.name}
          </a>
        </dd>
        <dt>Source</dt>
        <dd>
          {d.source.kind} <code title={d.source.ref}>{shortRef(d.source.ref)}</code> from <code>{d.source.where}</code>
        </dd>
        <dt>Plan</dt>
        <dd>
          {/* Its first 12 hex, the whole digest to copy (UI audit #167). */}
          <code class="deploy-digest" title={d.plan} data-deploy-plan={d.plan}>
            {shortRef(d.plan)}
          </code>{" "}
          <CopyButton text={d.plan} />
        </dd>
        <dt>Planned</dt>
        <dd>
          by {planner(d)}, <Time at={d.created_at} />
        </dd>
      </dl>
      {d.plan_text && !deciding ? (
        <details class="deploy-op" open={planOpen ?? !link} data-deploy-plan-text>
          <summary>{d.roll_back ? "The plan: a rollback" : "The plan"}</summary>
          <pre>{d.plan_text}</pre>
        </details>
      ) : null}
      {deciding ? null : (
        <details class="deploy-op">
          <summary>The operation an approval signs</summary>
          <pre data-deploy-op>{d.operation.body}</pre>
          <p class="dim">
            Its hash <code class="deploy-digest">{d.operation.hash}</code>
          </p>
        </details>
      )}
      {d.stale ? (
        <p class="deploy-note" data-deploy-stale={d.stale.why}>
          {staleNote(d.stale, d.environment.name)}{" "}
          <a href={`/deploys/${d.stale.deploy}`}>{d.stale.deploy.slice(0, 12)}</a>
        </p>
      ) : null}
      {after && after.d.id === given.id ? (
        <p class="deploy-note" data-deploy-done>
          {after.said}
        </p>
      ) : d.state === "pending" ? (
        <DeployDecide key={`${d.id} ${d.operation.hash}`} d={d} onDecided={decided} />
      ) : null}
    </article>
  );
}

/** Why a deploy that won't be applied ended: its last event's reason. */
export function endedWhy(d: Pick<Deploy, "state" | "events">): string | null {
  if (!["failed", "canceled", "expired", "denied"].includes(d.state)) return null;
  const last = d.events.at(-1);
  return typeof last?.detail?.reason === "string" ? last.detail.reason : null;
}

/** A run's page, from a deploy's (UI audit #146). */
const runPath = (d: Pick<Deploy, "project">, run: string) => pathOf({ page: "run", project: d.project.id, run }, names);

/** `/deploys/:id`: the deploy, its approvals and its log. */
export function DeployPage({ id }: { id: string }) {
  const [d, setD] = useState<Deploy | null>(null);
  const [error, setError] = useState("");
  useEffect(() => {
    // Another deploy: the last one's never shows while this one loads.
    setD(null);
    setError("");
    let gone = false;
    get<Deploy>(`/api/deploys/${id}`).then(
      (x) => gone || setD(x),
      (e: Error) => gone || setError(e.message),
    );
    return () => void (gone = true);
  }, [id]);
  // The bar is in its project once that's known (UI audit #159).
  useEffect(() => void (d && where.deployIn(d.id, d.project.id)), [d]);
  if (error) return <section class="platform-section"><p class="control-error">{error}</p></section>;
  if (!d) return <p class="platform-section dim">Loading…</p>;
  const deploys = pathOf({ page: "deploys", project: d.project.id }, names);
  return (
    <section class="platform-section sw-page deploy-page" data-deploy-page={d.id}>
      <PageHeader
        title={shownTitle(d)}
        named
        back={{ href: deploys, label: "Deploys", onClick: (e) => follow(e, deploys) }}
        state={<DeployState state={d.state} />}
      />
      <DeployCard d={d} headless onDecided={setD} />
      {endedWhy(d) || (d.state === "failed" && d.run) ? (
        // Why it failed, and the run that has its log (UI audit #146).
        <p class="deploy-note" data-deploy-why>
          {stateText(d.state)}
          {endedWhy(d) ? `: ${endedWhy(d)}` : ""}
          {d.run ? (
            <>
              {" · "}
              <a href={runPath(d, d.run)} onClick={(e) => follow(e, runPath(d, d.run!))} data-deploy-run>
                its run's log
              </a>
            </>
          ) : null}
        </p>
      ) : null}
      {d.approvals.length > 0 ? (
        <>
          <h2>Approvals</h2>
          <ul class="platform-list deploy-approvals">
            {d.approvals.map((a) => (
              <li key={a.id} data-deploy-approval={a.decision}>
                {a.decision === "once" ? "Approved" : "Denied"} by {a.name || "a deleted account"}
                {a.v === 2 ? " with a passkey" : <>{` on ${a.device_name}`}{a.passkey ? ", with a passkey" : ""}</>}, <Time at={a.at} />
                {/* Only while it can still be applied (UI audit #167). */}
                {a.decision === "once" && d.state === "approved" ? <span class="dim">
                    {" "}
                    · expires <Time at={a.expires} />
                  </span> : null}
              </li>
            ))}
          </ul>
        </>
      ) : null}
      <h2>
        Log{" "}
        <span class="dim" data-deploy-log>
          {d.log.verified !== undefined ? "the environment's chain checks out" : `the chain breaks at ${d.log.broken_at}`}
        </span>
      </h2>
      <ol class="platform-list deploy-events">
        {d.events.map((e) => (
          <li key={e.seq}>
            {/* In the header's words (UI audit #183). */}
            <span class="dim">#{e.seq}</span> <span data-deploy-event={e.kind}>{stateText(deployEventState(e.kind))}</span>
            {typeof e.detail?.reason === "string" ? `: ${e.detail.reason}` : ""}{" "}
            <span class="dim">
              by{" "}
              {e.run ? (
                <a href={runPath(d, e.run)} onClick={(ev) => follow(ev, runPath(d, e.run!))}>
                  run <code>{e.run}</code>
                </a>
              ) : (
                e.by
              )}
              , <Time at={e.at} />
            </span>
          </li>
        ))}
      </ol>
    </section>
  );
}

/** A deploy in an environment's history: a table row, its state's chip,
 * its short ref opening its page (the whole row does), whether it's a
 * rollback, and when it last moved. */
export function DeployRow({ d }: { d: DeploySummary }) {
  const path = `/deploys/${d.id}`;
  const at = lastMoved(d);
  return (
    <tr class={`deploy-row ${toneClass(deployTone(d.state))}`} data-deploy-row={d.id} data-deploy-state={d.state} onClick={(e) => followRow(e, path)}>
      <td>
        <DeployState state={d.state} />
      </td>
      <td>
        {/* An image's digest says so; a commit is the default (UI audit #166). */}
        {d.source.kind !== "commit" && <span class="deploy-source-kind">{d.source.kind === "artifact" ? "digest" : d.source.kind}</span>}
        <a href={path} onClick={(e) => follow(e, path)} title={`${d.source.kind} ${d.source.ref} from ${d.source.where}, planned by ${planner(d)}`}>
          <code>{shortRef(d.source.ref)}</code>
        </a>
      </td>
      <td>
        {d.roll_back ? (
          <span class="deploy-roll-back" data-deploy-roll-back>
            rollback
          </span>
        ) : null}
      </td>
      <td class="dim">
        <Time at={at} />
      </td>
    </tr>
  );
}

/** The deploys waiting for the reader's approval: null while asking. */
export function useWaitingDeploys(): Deploy[] | null {
  const [list, setList] = useState<Deploy[] | null>(null);
  useEffect(() => {
    get<{ deploys: Deploy[] }>("/api/deploys/waiting").then(
      (v) => {
        setList(v.deploys);
        // The bar's Inbox counts them too (#468).
        noteWaitingDeploys(v.deploys.length);
      },
      () => setList([]),
    );
  }, []);
  return list;
}

/** Those deploys as cards: on the inbox, first. */
export function WaitingDeploys({ list }: { list: Deploy[] | null }) {
  if (!list?.length) return null;
  return (
    <div data-inbox-group="deploys">
      <h2>
        Deploys waiting for you <span class="dim">{list.length}</span>
      </h2>
      <div class="deploy-cards sw-cards">
        {list.map((d) => (
          <DeployCard key={d.id} d={d} link />
        ))}
      </div>
    </div>
  );
}
