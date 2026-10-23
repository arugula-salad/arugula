// A project's environments (M252), in its settings: each one's driver,
// settings and secrets' names, and (owners) making one and setting its
// secrets. A secret's value goes from this page to the platform once,
// sealed with the project's key there, and is never shown or read back:
// only a deploy's run gets it, on its own box, while it runs. Setting one
// takes a browser's session (a CLI's login or an agent is refused). Owners
// and editors plan a deploy to one ("Deploy…"); approving it stays an
// owner's, with a passkey, on control's page. Every member sees each
// environment's latest deploys, whatever became of them, and reaches each
// one's page from there, not only from its push.

import { Fragment } from "preact";
import { useEffect, useState } from "preact/hooks";
import { platformFetch } from "../../shared/maintenance";
import { holdable, historyPath, secretNote, environmentBody, type DeployHistory, type Driver, type Environment } from "../deploys-text";
import { names } from "../links";
import { DeployRow } from "../pages/deploy";
import { follow, pathOf } from "../router";
import type { PlatformProject } from "../session";

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

/** The reader's role in a project: a personal project is its owner's. */
export const roleIn = (p: PlatformProject) => p.team?.role ?? "owner";

export function Environments({ p }: { p: PlatformProject }) {
  const [envs, setEnvs] = useState<Environment[] | null>(null);
  const [drivers, setDrivers] = useState<Driver[]>([]);
  const [making, setMaking] = useState(false);
  const [err, setErr] = useState("");
  const base = `/api/projects/${encodeURIComponent(p.id)}`;
  const load = () =>
    Promise.all([call<{ environments: Environment[] }>("GET", `${base}/environments`), call<{ drivers: Driver[] }>("GET", `${base}/drivers`)]).then(
      ([e, d]) => {
        setEnvs(e.environments);
        setDrivers(d.drivers);
      },
      (e: Error) => setErr(e.message),
    );
  useEffect(() => void load(), [p.id]);
  const role = roleIn(p);
  const owner = role === "owner";
  const plans = role !== "viewer";
  const deployPath = (env?: string) => pathOf({ page: "new-deploy", project: p.id, ...(env ? { env } : {}) }, names);
  return (
    <div data-environments>
      <h2>
        Environments
        {plans && envs?.length ? (
          <>
            {" "}
            <a class="button deploy-new" href={deployPath()} data-deploy-new onClick={(e) => follow(e, deployPath())}>
              Deploy…
            </a>
          </>
        ) : null}
      </h2>
      <p class="dim">
        Where the project's deploys go. Each has a driver that applies a deploy, the driver's settings, and the secrets it holds. A deploy is planned by an
        owner or editor (or their agent), and applied only once an owner approves it on their phone or in a browser, with a passkey.
      </p>
      {err && <p class="control-error">{err}</p>}
      {envs?.length === 0 && <p class="dim">None yet.</p>}
      <div class="deploy-cards">
        {(envs ?? []).map((env) => (
          <EnvironmentCard
            key={env.id}
            env={env}
            driver={drivers.find((d) => d.name === env.driver)}
            owner={owner}
            plans={plans}
            deployPath={deployPath(env.id)}
          />
        ))}
      </div>
      {owner ? (
        making ? (
          <NewEnvironment
            base={base}
            drivers={drivers}
            done={(made) => {
              setMaking(false);
              if (made) void load();
            }}
          />
        ) : (
          <p>
            <button class="control-linkish" data-environment-new onClick={() => setMaking(true)} disabled={!drivers.length}>
              New environment
            </button>
          </p>
        )
      ) : (
        <p class="dim">A project's owners make its environments and set their secrets.</p>
      )}
    </div>
  );
}

function EnvironmentCard({ env, driver, owner, plans, deployPath }: { env: Environment; driver?: Driver; owner: boolean; plans: boolean; deployPath: string }) {
  const settings = Object.entries(env.settings);
  const minted = (driver?.secrets ?? []).filter((s) => s.minted && !env.secret_names.includes(s.name));
  return (
    <article class="deploy-card deploy-env" data-environment={env.name}>
      <header class="deploy-card-head">
        <h2>{env.name}</h2>
        <span class="deploy-state">{env.driver}</span>
      </header>
      {settings.length > 0 && (
        <dl class="deploy-facts">
          {settings.map(([k, v]) => (
            <Fragment key={k}>
              <dt>{k}</dt>
              <dd>
                <code>{v}</code>
              </dd>
            </Fragment>
          ))}
        </dl>
      )}
      {env.secret_names.length > 0 && (
        <div class="deploy-secrets">
          <h3>Secrets</h3>
          {owner && <p class="dim">Values are never shown here again, or read back: set one to replace it.</p>}
          {env.secret_names.map((n) =>
            owner ? (
              <SecretRow key={n} env={env.id} name={n} />
            ) : (
              <p key={n}>
                <code>{n}</code>
              </p>
            ),
          )}
        </div>
      )}
      {minted.length > 0 && (
        <p class="dim" data-env-minted>
          {minted.map((s) => `${s.name}: ${secretNote(s)}`).join(". ")}.
        </p>
      )}
      <RecentDeploys project={env.project} env={env.id} />
      {plans && (
        <div class="deploy-actions">
          <a class="button" href={deployPath} data-environment-deploy={env.name} onClick={(e) => follow(e, deployPath)}>
            Deploy…
          </a>
        </div>
      )}
    </article>
  );
}

/** How many of an environment's deploys it shows at first, and how many
 * more each "Older" asks for. */
const RECENT = 5;
const OLDER = 20;

/** An environment's latest deploys, whatever became of them, newest
 * first: each opens its own page. */
function RecentDeploys({ project, env }: { project: string; env: string }) {
  const [page, setPage] = useState<DeployHistory | null>(null);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState("");
  useEffect(() => {
    call<DeployHistory>("GET", historyPath(project, env, RECENT)).then(setPage, (e: Error) => setErr(e.message));
  }, [project, env]);
  const older = async () => {
    if (!page?.next) return;
    setBusy(true);
    try {
      const more = await call<DeployHistory>("GET", historyPath(project, env, OLDER, page.next));
      setPage({ deploys: [...page.deploys, ...more.deploys], next: more.next });
    } catch (e) {
      setErr((e as Error).message);
    } finally {
      setBusy(false);
    }
  };
  if (err) return <p class="control-error">{err}</p>;
  if (!page) return null;
  return (
    <div class="deploy-history" data-env-deploys={env}>
      <h3>Deploys</h3>
      {page.deploys.length === 0 ? (
        <p class="dim">None yet.</p>
      ) : (
        <ol class="platform-list">
          {page.deploys.map((d) => (
            <DeployRow key={d.id} d={d} />
          ))}
        </ol>
      )}
      {page.next && (
        <button class="control-linkish" disabled={busy} data-env-deploys-older onClick={() => void older()}>
          Older
        </button>
      )}
    </div>
  );
}

/** One secret: a password box, sent once, then cleared. */
function SecretRow({ env, name }: { env: string; name: string }) {
  const [value, setValue] = useState("");
  const [busy, setBusy] = useState(false);
  const [said, setSaid] = useState<{ ok: boolean; text: string } | null>(null);
  const save = async (e: Event) => {
    e.preventDefault();
    if (!value) return;
    setBusy(true);
    setSaid(null);
    try {
      await call("POST", `/api/environments/${encodeURIComponent(env)}/secrets/${encodeURIComponent(name)}`, { value });
      setSaid({ ok: true, text: "Set." });
    } catch (err) {
      setSaid({ ok: false, text: (err as Error).message });
    } finally {
      // Never kept on the page, whatever happened.
      setValue("");
      setBusy(false);
    }
  };
  return (
    <form class="deploy-secret" data-env-secret={name} onSubmit={(e) => void save(e)}>
      <label>
        <code>{name}</code>
        <input
          type="password"
          autoComplete="new-password"
          spellcheck={false}
          placeholder="New value"
          aria-label={`${name}'s value`}
          value={value}
          data-env-secret-value={name}
          onInput={(e) => setValue(e.currentTarget.value)}
        />
      </label>
      <button disabled={busy || !value} data-env-secret-save={name}>
        Set
      </button>
      {said && (
        <span class={said.ok ? "dim" : "control-error"} role="status" data-env-secret-said={name}>
          {said.text}
        </span>
      )}
    </form>
  );
}

function NewEnvironment({ base, drivers, done }: { base: string; drivers: Driver[]; done: (made: boolean) => void }) {
  const [name, setName] = useState("");
  const [driver, setDriver] = useState(drivers[0]?.name ?? "");
  const [values, setValues] = useState<Record<string, string>>({});
  const d = drivers.find((x) => x.name === driver);
  const [secrets, setSecrets] = useState<string[]>([]);
  const [err, setErr] = useState("");
  const [busy, setBusy] = useState(false);
  // A driver's own secrets, all held to begin with.
  useEffect(() => {
    setSecrets(
      d
        ? holdable(d)
            .filter((s) => s.minted === null)
            .map((s) => s.name)
        : [],
    );
    setValues({});
  }, [driver]);
  const submit = async (e: Event) => {
    e.preventDefault();
    const b = environmentBody(name, d, values, secrets);
    if ("error" in b) return setErr(b.error);
    setBusy(true);
    setErr("");
    try {
      await call("POST", `${base}/environments`, b.body);
      done(true);
    } catch (x) {
      setErr((x as Error).message);
    } finally {
      setBusy(false);
    }
  };
  return (
    <form class="deploy-form" aria-label="New environment" data-environment-form onSubmit={(e) => void submit(e)}>
      <h3>New environment</h3>
      <label>
        Name
        <input required placeholder="production" value={name} data-environment-name onInput={(e) => setName(e.currentTarget.value)} />
      </label>
      <label>
        Driver
        <select value={driver} data-environment-driver onChange={(e) => setDriver(e.currentTarget.value)}>
          {drivers.map((x) => (
            <option key={x.name} value={x.name}>
              {x.name}
            </option>
          ))}
        </select>
      </label>
      {d?.settings.map((f) => (
        <label key={f.name}>
          <span>
            <code>{f.name}</code> <span class="dim">{f.required ? "" : f.default ? `(default ${f.default})` : "(optional)"}</span>
          </span>
          {f.choices.length ? (
            <select value={values[f.name] ?? ""} data-environment-setting={f.name} onChange={(e) => setValues({ ...values, [f.name]: e.currentTarget.value })}>
              <option value="">{f.default ? `${f.default} (default)` : "no"}</option>
              {f.choices
                .filter((c) => c !== f.default)
                .map((c) => (
                  <option key={c} value={c}>
                    {c}
                  </option>
                ))}
            </select>
          ) : (
            <input
              required={f.required}
              value={values[f.name] ?? ""}
              data-environment-setting={f.name}
              onInput={(e) => setValues({ ...values, [f.name]: e.currentTarget.value })}
            />
          )}
          <span class="dim deploy-about">{f.about}</span>
        </label>
      ))}
      {d && d.secrets.length > 0 && (
        <fieldset class="deploy-secret-names">
          <legend>Secrets it holds</legend>
          {d.secrets.map((s) =>
            s.minted === "always" ? (
              <p key={s.name} class="dim">
                <code>{s.name}</code>: {secretNote(s)}
              </p>
            ) : (
              <label key={s.name} class="control-confirm">
                <input
                  type="checkbox"
                  checked={secrets.includes(s.name)}
                  data-environment-secret={s.name}
                  onChange={(e) => setSecrets(e.currentTarget.checked ? [...secrets, s.name] : secrets.filter((n) => n !== s.name))}
                />{" "}
                <code>{s.name}</code>
                {s.minted ? <span class="dim">: {secretNote(s)}</span> : null}
              </label>
            ),
          )}
          <p class="dim">You set their values once it's made.</p>
        </fieldset>
      )}
      {err && (
        <p role="alert" class="control-error" data-environment-error>
          {err}
        </p>
      )}
      <div class="prompt-buttons">
        <button type="button" onClick={() => done(false)}>
          Cancel
        </button>
        <button class="primary" disabled={busy} data-environment-make>
          Make it
        </button>
      </div>
    </form>
  );
}
