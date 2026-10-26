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
//
// Each environment is a card in the swarm's language (../swarm.css): its
// bar is its latest deploy's state (none yet is muted), its driver a tag,
// each secret a chip saying whether the environment holds it or the
// platform mints it per run, and its deploys a compact table.

import { useEffect, useState } from "preact/hooks";
import { platformFetch } from "../../shared/maintenance";
import { holdable, historyPath, secretNote, environmentBody, type DeployHistory, type Driver, type Environment } from "../deploys-text";
import { names } from "../links";
import { DeployRow } from "../pages/deploy";
import { follow, pathOf } from "../router";
import type { PlatformProject } from "../session";
import { deployTone, toneClass } from "../swarm";
import { Section } from "./section";

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
  const deployPath = (env: string) => pathOf({ page: "new-deploy", project: p.id, env }, names);
  return (
    <Section
      id="environments"
      title="Environments"
      count={envs?.length}
      hooks={{ "data-environments": "" }}
      lede="Where the project's deploys go. Nothing is applied until an owner approves it with a passkey."
      more={
        <p>
          Each has a driver that applies a deploy, the driver's settings, and the secrets it holds. An owner or editor (or their agent) plans a
          deploy with Deploy…, and it's applied only once an owner approves it on their phone or in a browser, with a passkey. A secret's value
          is never shown here again, or read back: only a deploy's run gets it. Set one to replace it.
        </p>
      }
    >
      {err && <p class="control-error">{err}</p>}
      {envs?.length === 0 && <p class="dim">None yet.</p>}
      {envs?.length ? (
        <div class="sw-cards settings-cards env-cards">
          {envs.map((env) => (
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
      ) : null}
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
          <button class="secondary settings-new" data-environment-new onClick={() => setMaking(true)} disabled={!drivers.length}>
            New environment
          </button>
        )
      ) : (
        <p class="dim">A project's owners make its environments and set their secrets.</p>
      )}
    </Section>
  );
}

function EnvironmentCard({ env, driver, owner, plans, deployPath }: { env: Environment; driver?: Driver; owner: boolean; plans: boolean; deployPath: string }) {
  const settings = Object.entries(env.settings);
  const minted = (driver?.secrets ?? []).filter((s) => s.minted && !env.secret_names.includes(s.name));
  const history = useHistory(env.project, env.id);
  // The bar: its latest deploy's state, muted until it has one.
  const latest = history.page?.deploys[0];
  return (
    <article class={`sw-card env-card ${toneClass(latest ? deployTone(latest.state) : "muted")}`} data-environment={env.name}>
      <div class="sw-head">
        <b class="sw-name">{env.name}</b>
        <span class="settings-tag" title="Its driver: what applies its deploys">
          {env.driver}
        </span>
        {plans && (
          <a class="secondary small env-deploy" href={deployPath} data-environment-deploy={env.name} data-deploy-new onClick={(e) => follow(e, deployPath)}>
            Deploy…
          </a>
        )}
      </div>
      {settings.length > 0 && (
        <dl class="env-facts">
          {settings.map(([k, v]) => (
            <div key={k}>
              <dt>{k}</dt>
              <dd>
                <code>{v}</code>
              </dd>
            </div>
          ))}
        </dl>
      )}
      {env.secret_names.length + minted.length > 0 && (
        <div class="env-part">
          <span class="env-label">Secrets</span>
          <ul class="env-secrets">
            {env.secret_names.map((n) =>
              owner ? (
                <SecretRow key={n} env={env.id} name={n} />
              ) : (
                <li key={n}>
                  <SecretChip name={n} />
                </li>
              ),
            )}
            {minted.map((s) => (
              <li key={s.name}>
                <SecretChip name={s.name} minted={secretNote(s)} />
              </li>
            ))}
          </ul>
        </div>
      )}
      <RecentDeploys env={env.id} history={history} />
    </article>
  );
}

/** A secret's chip: its name, then held (the environment has a value) or
 * minted per run (the platform makes one for each run; the tooltip says
 * whether the environment's own would win). */
function SecretChip({ name, minted }: { name: string; minted?: string }) {
  return minted ? (
    <span class="settings-chip minted" title={`${name}: ${minted}`} data-env-minted>
      <code>{name}</code>
      <span>minted per run</span>
    </span>
  ) : (
    <span class="settings-chip" title={`${name}: the environment holds its value`}>
      <code>{name}</code>
      <span>held</span>
    </span>
  );
}

/** How many of an environment's deploys it shows at first, and how many
 * more each "Older" asks for. */
const RECENT = 5;
const OLDER = 20;

type History = ReturnType<typeof useHistory>;

/** An environment's latest deploys, whatever became of them, newest first,
 * and asking for older ones. The card's bar reads the newest. */
function useHistory(project: string, env: string) {
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
  return { page, busy, err, older };
}

/** Those deploys as a table: each row opens its own page. */
function RecentDeploys({ env, history: { page, busy, err, older } }: { env: string; history: History }) {
  if (err) return <p class="control-error">{err}</p>;
  if (!page) return null;
  return (
    <div class="env-part" data-env-deploys={env}>
      <span class="env-label">Deploys</span>
      {page.deploys.length === 0 ? (
        <p class="dim">None yet.</p>
      ) : (
        <table class="settings-table deploy-table">
          <thead class="settings-hidden">
            <tr>
              <th scope="col">State</th>
              <th scope="col">Commit</th>
              <th scope="col">Rollback</th>
              <th scope="col">When</th>
            </tr>
          </thead>
          <tbody>
            {page.deploys.map((d) => (
              <DeployRow key={d.id} d={d} />
            ))}
          </tbody>
        </table>
      )}
      {page.next && (
        <button class="link env-older" disabled={busy} data-env-deploys-older onClick={() => void older()}>
          Older
        </button>
      )}
    </div>
  );
}

/** One secret the environment holds, for an owner: its chip, and a
 * password box to replace its value, sent once, then cleared. */
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
    <li class="env-secret-row">
      <form class="env-secret" data-env-secret={name} onSubmit={(e) => void save(e)}>
        <SecretChip name={name} />
        <input
          type="password"
          autoComplete="new-password"
          spellcheck={false}
          placeholder="New value"
          aria-label={`${name}'s value`}
          title="Values are never shown again, or read back: set one to replace it."
          value={value}
          data-env-secret-value={name}
          onInput={(e) => setValue(e.currentTarget.value)}
        />
        <button class="primary small" disabled={busy || !value} data-env-secret-save={name}>
          Set
        </button>
        {said && (
          <span class={said.ok ? "dim" : "control-error"} role="status" data-env-secret-said={name}>
            {said.text}
          </span>
        )}
      </form>
    </li>
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
        <button class="secondary" type="button" onClick={() => done(false)}>
          Cancel
        </button>
        <button class="primary" disabled={busy} data-environment-make>
          Make it
        </button>
      </div>
    </form>
  );
}
