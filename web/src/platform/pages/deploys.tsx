// `/p/:project/deploys`: a project's deploys, every environment's, newest
// first (`GET /api/projects/{project}/deploys`, a page at a time), each
// row opening its own page (./deploy). Its bar has a place for it
// (Discovery 5): before, the only way in was Project settings ›
// Environments › Deploy…. With none yet it says what a deploy is and offers
// the next step: Deploy… where there's an environment and the reader plans,
// else Make an environment for an owner, else who does.

import { useEffect, useState } from "preact/hooks";
import { platformFetch, refusalOf } from "../../shared/maintenance";
import { PageHeader } from "../../shared/ui/page-header";
import { Time } from "../../shared/ui/time";
import { useSubscribe } from "../../shared/ui/hooks";
import { names } from "../links";
import { follow, pathOf } from "../router";
import { session } from "../session";
import { historyPath, lastMoved, rowNote, shortRef, type DeployHistory, type DeploySummary, type Environment } from "../deploys-text";
import { deployTone, toneClass } from "../swarm";
import { roleIn } from "../settings/environments";
import { DeployState, planner } from "./deploy";
import { Missing } from "./project";

async function get<T>(path: string): Promise<T> {
  const r = await platformFetch(path, { credentials: "same-origin" });
  const v = (await r.json().catch(() => ({}))) as T & { error?: string; reason?: string };
  if (!r.ok) throw new Error(refusalOf(r.status, v));
  return v;
}

/** What a deploy is, in one sentence: the empty page's, and the project
 * home's. */
export const DEPLOYS_MEAN = "A deploy puts a commit on one of the project's environments, once an owner approves it with a passkey.";

const PAGE = 25;

export function DeploysPage({ project }: { project: string }) {
  useSubscribe((fn) => session.subscribe(fn));
  const p = session.project(project);
  const [envs, setEnvs] = useState<Environment[] | null>(null);
  const [page, setPage] = useState<DeployHistory | null>(null);
  const [err, setErr] = useState("");
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    let live = true;
    const base = `/api/projects/${encodeURIComponent(project)}`;
    Promise.all([get<{ environments: Environment[] }>(`${base}/environments`), get<DeployHistory>(historyPath(project, undefined, PAGE))]).then(
      ([e, d]) => {
        if (!live) return;
        setEnvs(e.environments);
        setPage(d);
      },
      (e: Error) => live && setErr(e.message),
    );
    return () => void (live = false);
  }, [project]);
  if (!p) return <Missing what="That project isn't one of yours (or it's gone)." />;
  const role = roleIn(p);
  const plans = role !== "viewer";
  const deployPath = pathOf({ page: "new-deploy", project: p.id }, names);
  const envPath = `${pathOf({ page: "project-settings", project: p.id }, names)}#environments`;
  const older = async () => {
    if (!page?.next) return;
    setBusy(true);
    try {
      const more = await get<DeployHistory>(historyPath(p.id, undefined, PAGE, page.next));
      setPage({ deploys: [...page.deploys, ...more.deploys], next: more.next });
    } catch (e) {
      setErr((e as Error).message);
    }
    setBusy(false);
  };
  return (
    <section class="platform-section sw-page deploys-page" data-deploys={p.id}>
      <PageHeader
        title="Deploys"
        actions={
          plans &&
          !!envs?.length && (
            <a class="primary" href={deployPath} data-deploys-new onClick={(e) => follow(e, deployPath)}>
              Deploy…
            </a>
          )
        }
        sub={
          <>
            Every environment's, newest first. Environments are in the project's{" "}
            <a href={envPath} onClick={(e) => follow(e, envPath)}>
              settings
            </a>
            .
          </>
        }
      />
      {err && (
        <p class="control-error" role="alert">
          {err}
        </p>
      )}
      {!err && (!page || !envs) && <p class="dim">Loading…</p>}
      {page && envs && page.deploys.length === 0 && (
        <div class="platform-empty" data-deploys-none>
          <p>{DEPLOYS_MEAN}</p>
          {envs.length === 0 ? (
            role === "owner" ? (
              <>
                <p class="dim">It needs an environment to go to first: where it goes, and the driver that applies it.</p>
                <a class="primary" href={envPath} data-deploys-make-env onClick={(e) => follow(e, envPath)}>
                  Make an environment
                </a>
              </>
            ) : (
              <p class="dim" data-deploys-why>
                It needs an environment first, and a project's owners make those.
              </p>
            )
          ) : plans ? (
            <a class="primary" href={deployPath} data-deploys-start onClick={(e) => follow(e, deployPath)}>
              Deploy…
            </a>
          ) : (
            <p class="dim" data-deploys-why>
              A team's owners and editors plan deploys; you can read them here.
            </p>
          )}
        </div>
      )}
      {!!page?.deploys.length && (
        <table class="settings-table deploy-table deploys-table" data-deploys-list>
          <thead>
            <tr>
              <th scope="col">State</th>
              <th scope="col">Environment</th>
              <th scope="col">Source</th>
              <th scope="col">Planned by</th>
              <th scope="col">When</th>
            </tr>
          </thead>
          <tbody>
            {page.deploys.map((d) => (
              <Row key={d.id} d={d} />
            ))}
          </tbody>
        </table>
      )}
      {page?.next && (
        <button class="link env-older" disabled={busy} data-deploys-older onClick={() => void older()}>
          Older
        </button>
      )}
    </section>
  );
}

function Row({ d }: { d: DeploySummary }) {
  const path = `/deploys/${d.id}`;
  return (
    <tr class={`deploy-row ${toneClass(deployTone(d.state))}`} data-deploy-row={d.id} data-deploy-state={d.state}>
      <td>
        <DeployState state={d.state} />
      </td>
      <td>{d.environment.name}</td>
      <td>
        {d.source.kind !== "commit" && <span class="deploy-source-kind">{d.source.kind === "artifact" ? "digest" : d.source.kind}</span>}
        <a href={path} onClick={(e) => follow(e, path)} title={`${d.source.kind} ${d.source.ref} from ${d.source.where}`}>
          <code>{shortRef(d.source.ref)}</code>
        </a>
        {d.roll_back ? (
          <span class="deploy-roll-back" data-deploy-roll-back>
            {" "}
            rollback
          </span>
        ) : null}
        {rowNote(d) ? (
          <span class="deploy-row-note dim" title={rowNote(d)} data-deploy-row-note>
            {rowNote(d)}
          </span>
        ) : null}
      </td>
      <td class="dim">{planner(d)}</td>
      <td class="dim">
        <Time at={lastMoved(d)} />
      </td>
    </tr>
  );
}
