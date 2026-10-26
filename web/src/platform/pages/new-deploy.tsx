// `/p/:project/deploys/new`: "Deploy…" (M252). An owner or editor picks
// one of the project's environments, a commit by its full id and the repo
// it's in (the project's linked repos offered), and whether it's a roll
// back; the platform plans it from the environment's driver and answers
// the plan's text, which is what an owner reads before they approve. Then
// "Ask for approval" pushes each owner a card. Approving isn't here: an
// owner approves on control's page, on their phone or in a browser, with a
// passkey (the deploy's card links there).

import { useEffect, useState } from "preact/hooks";
import type { LinkedRepos } from "../../shared/proto.gen";
import { platformFetch } from "../../shared/maintenance";
import { useSubscribe } from "../../shared/ui/hooks";
import { PageHeader } from "../../shared/ui/page-header";
import { historyPath, planBody, repoFor, type DeployHistory, type Driver, type Environment } from "../deploys-text";
import { names } from "../links";
import { follow, pathOf, REPO } from "../router";
import { session } from "../session";
import { roleIn } from "../settings/environments";
import { DeployCard, type Deploy } from "./deploy";
import { Missing } from "./project";

async function call<T>(method: string, path: string, body?: unknown): Promise<T> {
  const r = await platformFetch(path, {
    method,
    credentials: "same-origin",
    headers: body === undefined ? {} : { "Content-Type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const v = await r.json().catch(() => ({}));
  if (!r.ok) throw new Error((v as { error?: string }).error ?? `Request failed (${r.status})`);
  return v as T;
}

export function NewDeployPage({ project, env }: { project: string; env?: string }) {
  useSubscribe((fn) => session.subscribe(fn));
  const p = session.project(project);
  const [envs, setEnvs] = useState<Environment[] | null>(null);
  const [drivers, setDrivers] = useState<Driver[]>([]);
  const [repos, setRepos] = useState<string[]>([]);
  const [environment, setEnvironment] = useState(env ?? "");
  const [commit, setCommit] = useState("");
  const [repo, setRepo] = useState("");
  // A repo typed by hand stays when the environment changes.
  const [typed, setTyped] = useState(false);
  // Where each environment's last deploy came from (null: none), for the
  // repo it offers (UI audit #147).
  const [lastFrom, setLastFrom] = useState<Record<string, string | null>>({});
  const [rollBack, setRollBack] = useState(false);
  const [planned, setPlanned] = useState<Deploy | null>(null);
  const [err, setErr] = useState("");
  const [busy, setBusy] = useState(false);
  const base = `/api/projects/${encodeURIComponent(project)}`;
  useEffect(() => {
    call<{ environments: Environment[] }>("GET", `${base}/environments`).then(
      (v) => {
        setEnvs(v.environments);
        if (!v.environments.some((e) => e.id === environment)) setEnvironment(v.environments[0]?.id ?? "");
      },
      (e: Error) => setErr(e.message),
    );
    call<{ drivers: Driver[] }>("GET", `${base}/drivers`).then(
      (v) => setDrivers(v.drivers),
      () => {},
    );
    call<LinkedRepos>("GET", `${base}/repos`).then(
      (v) => {
        const linked = v.repos.filter((r) => r.purge_after === null).map((r) => r.full_name);
        setRepos(linked);
      },
      () => {},
    );
  }, [project]);
  useEffect(() => {
    if (!environment || environment in lastFrom) return;
    const known = (where: string | null) => setLastFrom((m) => ({ ...m, [environment]: where }));
    call<DeployHistory>("GET", historyPath(project, environment, 1)).then(
      (v) => {
        const s = v.deploys[0]?.source;
        known(s?.kind === "commit" && REPO.test(s.where) ? s.where : null);
      },
      () => known(null),
    );
  }, [project, environment]);
  const envName = envs?.find((e) => e.id === environment)?.name;
  useEffect(() => {
    if (!typed && environment in lastFrom) setRepo(repoFor(envName, repos, lastFrom[environment]));
  }, [environment, envName, repos, lastFrom, typed]);
  if (!p) return <Missing what="That project isn't one of yours (or it's gone)." />;
  const settings = pathOf({ page: "project-settings", project: p.id }, names);
  const back = { href: settings, label: "Project settings", onClick: (e: MouseEvent) => follow(e, settings) };
  if (roleIn(p) === "viewer") {
    return (
      <section class="platform-section sw-page">
        <PageHeader title="Plan a deploy" back={back} />
        <p class="dim">A viewer reads the project's deploys, and doesn't plan them.</p>
      </section>
    );
  }
  const chosen = envs?.find((e) => e.id === environment);
  const rollsBack = drivers.find((d) => d.name === chosen?.driver)?.rolls_back ?? false;
  const act = async (f: () => Promise<Deploy>) => {
    setBusy(true);
    setErr("");
    try {
      setPlanned(await f());
    } catch (e) {
      setErr((e as Error).message);
    } finally {
      setBusy(false);
    }
  };
  const plan = (e: Event) => {
    e.preventDefault();
    const b = planBody(environment, commit, repo, rollsBack && rollBack, REPO);
    if ("error" in b) return setErr(b.error);
    void act(() => call<Deploy>("POST", `${base}/deploys`, b.body));
  };
  const ask = () => planned && void act(() => call<Deploy>("POST", `/api/deploys/${planned.id}/request`));
  return (
    <section class="platform-section sw-page deploy-new-page" data-deploy-new-page={p.id}>
      <PageHeader title="Plan a deploy" back={back} />
      {envs?.length === 0 ? (
        <p class="dim">The project has no environments yet: an owner makes one in its settings.</p>
      ) : planned ? null : (
        <form class="deploy-form" aria-label="Plan a deploy" data-deploy-plan-form onSubmit={plan}>
          <label>
            Environment
            <select value={environment} data-deploy-env onChange={(e) => setEnvironment(e.currentTarget.value)}>
              {(envs ?? []).map((x) => (
                <option key={x.id} value={x.id}>
                  {x.name} ({x.driver})
                </option>
              ))}
            </select>
          </label>
          <label>
            <span>
              Commit <span class="dim">(its full id: git rev-parse HEAD)</span>
            </span>
            <input
              class="deploy-commit"
              required
              spellcheck={false}
              autoComplete="off"
              placeholder="40 hex characters"
              value={commit}
              data-deploy-commit
              onInput={(e) => setCommit(e.currentTarget.value)}
            />
          </label>
          <label>
            Repo
            <input
              required
              list="deploy-repos"
              placeholder="OWNER/NAME"
              value={repo}
              data-deploy-repo
              onInput={(e) => {
                setRepo(e.currentTarget.value);
                setTyped(true);
              }}
            />
            <datalist id="deploy-repos">
              {repos.map((r) => (
                <option key={r} value={r} />
              ))}
            </datalist>
          </label>
          {rollsBack && (
            <label class="control-confirm">
              <input type="checkbox" checked={rollBack} data-deploy-roll-back onChange={(e) => setRollBack(e.currentTarget.checked)} /> Roll back: to this
              commit, which {chosen?.name} ran before, with what it ran then
            </label>
          )}
          <div class="prompt-buttons">
            <button class="primary" disabled={busy || !envs?.length} data-deploy-plan>
              Plan
            </button>
          </div>
        </form>
      )}
      {err && (
        <p role="alert" class="control-error" data-deploy-error>
          {err}
        </p>
      )}
      {planned && (
        <>
          <DeployCard d={planned} link planOpen />
          {planned.state === "planned" ? (
            <div class="deploy-actions">
              <button type="button" onClick={() => setPlanned(null)}>
                Change it
              </button>
              <button class="primary" disabled={busy} data-deploy-ask onClick={ask}>
                Ask for approval
              </button>
              <p class="dim deploy-why">Read the plan: it's what an owner approves. Asking sends each of the project's owners a card.</p>
            </div>
          ) : planned.state === "pending" ? (
            <p class="dim" data-deploy-asked>
              Asked. An owner approves it on their phone or in a browser, with a passkey{planned.may_approve ? ": you can, with Approve above" : ""}.
            </p>
          ) : null}
        </>
      )}
    </section>
  );
}
