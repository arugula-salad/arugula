// Deploys waiting for an owner (M218): a card per deploy, showing where it
// goes, what it deploys, the plan's digest and the operation an approval
// signs. `/deploys/:id` is one deploy with its approvals and log; the
// inbox lists the deploys waiting for the reader, and a project's
// environments their history (`DeployRow`).
//
// Approving needs a device key, and this page holds none ("The platform
// page is on app.arugula.io, not on the keys' origin"): Approve and Deny
// open the deploy on control's page (`#deploy=dep_…`), which shows the
// same operation, asks for a passkey and signs it there.

import { useEffect, useState } from "preact/hooks";
import { platformFetch, refusalOf } from "../../shared/maintenance";
import { deployEventState, deployStateText, plannerText } from "../../shared/deploys";
import { CopyButton } from "../../shared/ui/copy";
import { PageHeader } from "../../shared/ui/page-header";
import { StateChip } from "../../shared/ui/state-chip";
import { controlOrigin } from "../session";
import { names } from "../links";
import { follow, pathOf } from "../router";
import { lastMoved, shortRef, type DeploySummary } from "../deploys-text";
import { ago } from "../settings/agent-tokens";
import { deployTone, toneClass } from "../swarm";
import { where } from "../where";

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

export const when = (ms: number) => new Date(ms).toLocaleString(undefined, { month: "short", day: "numeric", hour: "numeric", minute: "2-digit" });

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

/** Where Approve and Deny go: the deploy on control's page. */
export const approveHref = (id: string) => `${controlOrigin()}/#deploy=${id}`;

/** One deploy's card. `link`: its heading opens the deploy's own page.
 * The plan's text is open on the deploy's own page, or with `planOpen`.
 * On the deploy's own page its heading and state are the page's
 * (`headless`); elsewhere it's a card in the swarm's language, its bar
 * its state's tone (UI audit #167). */
export function DeployCard({ d, link, planOpen, headless }: { d: Deploy; link?: boolean; planOpen?: boolean; headless?: boolean }) {
  const path = `/deploys/${d.id}`;
  const project = pathOf({ page: "project", project: d.project.id }, names);
  return (
    <article class={headless ? "deploy-card" : `deploy-card sw-card ${toneClass(deployTone(d.state))}`} data-deploy-card={d.id} data-deploy-state={d.state}>
      {!headless && (
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
          <DeployState state={d.state} />
        </header>
      )}
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
          by {planner(d)}, {when(d.created_at)}
        </dd>
      </dl>
      {d.plan_text ? (
        <details class="deploy-op" open={planOpen ?? !link} data-deploy-plan-text>
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
      {d.stale ? <p class="deploy-note">Another deploy was applied to {d.environment.name} after this was planned, so it can't be approved. Plan it again.</p> : null}
      {d.may_approve ? (
        <div class="deploy-actions">
          <a class="secondary" href={approveHref(d.id)} data-deploy-deny>
            Deny
          </a>
          <a class="primary" href={approveHref(d.id)} data-deploy-approve>
            Approve
          </a>
          <p class="dim deploy-why">Opens on control's page, where your device keys are. You confirm with a passkey there.</p>
        </div>
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
    get<Deploy>(`/api/deploys/${id}`).then(setD, (e: Error) => setError(e.message));
  }, [id]);
  // The bar is in its project once that's known (UI audit #159).
  useEffect(() => void (d && where.deployIn(d.id, d.project.id)), [d]);
  if (error) return <section class="platform-section"><p class="control-error">{error}</p></section>;
  if (!d) return <p class="platform-section dim">Loading…</p>;
  const settings = pathOf({ page: "project-settings", project: d.project.id }, names);
  return (
    <section class="platform-section sw-page deploy-page" data-deploy-page={d.id}>
      <PageHeader
        title={`Deploy to ${d.environment.name}`}
        named
        back={{ href: settings, label: "Project settings", onClick: (e) => follow(e, settings) }}
        state={<DeployState state={d.state} />}
      />
      <DeployCard d={d} headless />
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
                {a.decision === "once" ? "Approved" : "Denied"} by {a.name || "a deleted account"} on {a.device_name}
                {a.passkey ? ", with a passkey" : ""}, {when(a.at)}
                {/* Only while it can still be applied (UI audit #167). */}
                {a.decision === "once" && d.state === "approved" ? <span class="dim"> · good until {when(a.expires)}</span> : null}
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
              , {when(e.at)}
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
    <tr class={`deploy-row ${toneClass(deployTone(d.state))}`} data-deploy-row={d.id} data-deploy-state={d.state}>
      <td>
        <DeployState state={d.state} />
      </td>
      <td>
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
      <td class="dim" title={when(at)}>
        {ago(at)}
      </td>
    </tr>
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
      <div class="deploy-cards sw-cards">
        {list.map((d) => (
          <DeployCard key={d.id} d={d} link />
        ))}
      </div>
    </div>
  );
}
