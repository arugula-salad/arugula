// Deploys waiting for an owner (M218): a card per deploy, showing where it
// goes, what it deploys, the plan's digest and the operation an approval
// signs. `/deploys/:id` is one deploy with its approvals and log; the
// inbox lists the deploys waiting for the reader.
//
// Approving needs a device key, and this page holds none ("The platform
// page is on app.arugula.io, not on the keys' origin"): Approve and Deny
// open the deploy on control's page (`#deploy=dep_…`), which shows the
// same operation, asks for a passkey and signs it there.

import { useEffect, useState } from "preact/hooks";
import { platformFetch, refusalOf } from "../../shared/maintenance";
import { controlOrigin } from "../session";
import { follow } from "../router";

export interface Deploy {
  id: string;
  project: { id: string; name: string };
  environment: { id: string; name: string; driver: string };
  source: { kind: string; ref: string; where: string };
  plan: string;
  state: string;
  planned_by: string;
  planned_by_name: string;
  agent: { machine: string; machine_name: string } | null;
  created_at: number;
  operation: { body: string; hash: string };
  stale: { deploy: string; seq: number } | null;
  may_approve: boolean;
  approvals: {
    id: string;
    decision: "once" | "deny";
    name: string;
    account: string;
    device_name: string;
    at: number;
    expires: number;
    passkey: { id: string } | null;
  }[];
  events: { seq: number; kind: string; actor: string; by: string; at: number; hash: string }[];
  log: { verified?: number; broken_at?: number };
}

async function get<T>(path: string): Promise<T> {
  const r = await platformFetch(path, { credentials: "same-origin" });
  const v = (await r.json().catch(() => ({}))) as T & { error?: string; reason?: string };
  if (!r.ok) throw new Error(refusalOf(r.status, v));
  return v;
}

const when = (ms: number) => new Date(ms).toLocaleString(undefined, { month: "short", day: "numeric", hour: "numeric", minute: "2-digit" });

/** Who planned it: a person, or "<owner>'s agent on <machine>". */
const planner = (d: Deploy) => {
  const who = d.planned_by_name || "a deleted account";
  return d.agent ? `${who}'s agent on ${d.agent.machine_name || d.agent.machine}` : who;
};

const STATE: Record<string, string> = {
  planned: "Planned",
  pending: "Waiting for approval",
  approved: "Approved",
  denied: "Denied",
  canceled: "Canceled",
  expired: "Expired",
  running: "Being applied",
  succeeded: "Applied",
  failed: "Failed",
  stale: "Stale",
};

/** Where Approve and Deny go: the deploy on control's page. */
export const approveHref = (id: string) => `${controlOrigin()}/#deploy=${id}`;

/** One deploy's card. `link`: its heading opens the deploy's own page. */
export function DeployCard({ d, link }: { d: Deploy; link?: boolean }) {
  const path = `/deploys/${d.id}`;
  return (
    <article class="deploy-card" data-deploy-card={d.id} data-deploy-state={d.state}>
      <header class="deploy-card-head">
        <h2>
          {link ? (
            <a href={path} onClick={(e) => follow(e, path)}>
              Deploy to {d.environment.name}
            </a>
          ) : (
            <>Deploy to {d.environment.name}</>
          )}
        </h2>
        <span class={`deploy-state deploy-state-${d.state}`}>{STATE[d.state] ?? d.state}</span>
      </header>
      <dl class="deploy-facts">
        <dt>Project</dt>
        <dd>{d.project.name}</dd>
        <dt>Source</dt>
        <dd>
          {d.source.kind} <code title={d.source.ref}>{d.source.ref.slice(0, 12)}</code> from <code>{d.source.where}</code>
        </dd>
        <dt>Plan</dt>
        <dd>
          <code class="deploy-digest" data-deploy-plan>
            {d.plan}
          </code>
        </dd>
        <dt>Planned</dt>
        <dd>
          by {planner(d)}, {when(d.created_at)}
        </dd>
      </dl>
      <details class="deploy-op">
        <summary>The operation an approval signs</summary>
        <pre data-deploy-op>{d.operation.body}</pre>
        <p class="dim">
          Its hash <code class="deploy-digest">{d.operation.hash}</code>
        </p>
      </details>
      {d.stale ? <p class="deploy-note">Another deploy was applied to {d.environment.name} after this was planned, so it can't be approved. Plan it again.</p> : null}
      {d.may_approve ? (
        <div class="deploy-actions">
          <a class="button" href={approveHref(d.id)} data-deploy-deny>
            Deny
          </a>
          <a class="button primary" href={approveHref(d.id)} data-deploy-approve>
            Approve
          </a>
          <p class="dim deploy-why">Opens on control's page, where your device keys are. You confirm with a passkey there.</p>
        </div>
      ) : null}
    </article>
  );
}

/** `/deploys/:id`: the deploy, its approvals and its log. */
export function DeployPage({ id }: { id: string }) {
  const [d, setD] = useState<Deploy | null>(null);
  const [error, setError] = useState("");
  useEffect(() => {
    get<Deploy>(`/api/deploys/${id}`).then(setD, (e: Error) => setError(e.message));
  }, [id]);
  if (error) return <section class="platform-section"><p class="control-error">{error}</p></section>;
  if (!d) return <p class="platform-section dim">Loading…</p>;
  return (
    <section class="platform-section deploy-page" data-deploy-page={d.id}>
      <DeployCard d={d} />
      {d.approvals.length > 0 ? (
        <>
          <h2>Approvals</h2>
          <ul class="platform-list deploy-approvals">
            {d.approvals.map((a) => (
              <li key={a.id} data-deploy-approval={a.decision}>
                {a.decision === "once" ? "Approved" : "Denied"} by {a.name || "a deleted account"} on {a.device_name}
                {a.passkey ? ", with a passkey" : ""}, {when(a.at)}
                {a.decision === "once" ? <span class="dim">: good until {when(a.expires)}</span> : null}
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
            <span class="dim">#{e.seq}</span> {e.kind} <span class="dim">
              by {e.by}, {when(e.at)}
            </span>
          </li>
        ))}
      </ol>
    </section>
  );
}

/** The deploys waiting for the reader's approval: null while asking. */
export function useWaitingDeploys(): Deploy[] | null {
  const [list, setList] = useState<Deploy[] | null>(null);
  useEffect(() => {
    get<{ deploys: Deploy[] }>("/api/deploys/waiting").then(
      (v) => setList(v.deploys),
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
      <div class="deploy-cards">
        {list.map((d) => (
          <DeployCard key={d.id} d={d} link />
        ))}
      </div>
    </div>
  );
}
