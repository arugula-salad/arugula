// What a cloud workspace's page changes (Discovery 10), each through the
// route `arugula cloud` calls: Edit (`cloud edit`, `PUT …/spec`: repos,
// packages, setup, env names and size), Rebuild (`cloud rebuild`, `POST
// …/rebuild`), its public previews (`cloud preview`, `…/ports`), its
// secrets (`cloud secret`, `…/secrets`) and the project's inference keys
// (`cloud key`, `/api/projects/{project}/inference-keys`). Owners and
// editors change the workspace's; only a project's owners its keys. The
// others see each part read only, and why.
//
// This page holds no keys, so it can't ask the box for unpushed work
// before a rebuild as the CLI does: Rebuild says so, and gives the
// command that asks.

import { useEffect, useState } from "preact/hooks";
import { platformFetch } from "../../shared/maintenance";
import { ConfirmRemove } from "../../shared/ui/confirm";
import { CopyText } from "../../shared/ui/copy";
import { Modal } from "../../shared/ui/modal";
import { Time } from "../../shared/ui/time";
import { session, type PlatformProject } from "../session";
import { SIZES, sizeOf, type Size } from "../settings/workspace-size";
import { mayWrite, nameIn } from "./issues";

/** A workspace's spec, as the platform answers it (`hosted::Spec`). */
export interface Spec {
  repos?: string[];
  packages?: string[];
  setup?: string;
  env?: string[];
  finish?: string;
  finish_timeout_s?: number;
  size?: string;
}

/** As much of a workspace as this needs. */
export interface Workspace {
  id: string;
  name: string;
  state: string;
  spec: Spec;
}

async function call<T>(method: string, path: string, body?: unknown): Promise<T> {
  const r = await platformFetch(path, {
    method,
    credentials: "same-origin",
    headers: body === undefined ? {} : { "Content-Type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const v = (await r.json().catch(() => ({}))) as { error?: string };
  if (!r.ok) throw new Error(v.error ?? `Request failed (${r.status})`);
  return v as T;
}

const base = (p: string, w: string) => `/api/projects/${encodeURIComponent(p)}/workspaces/${encodeURIComponent(w)}`;

/** Why the reader can't change the workspace, or null: a team's viewer. */
const whyNot = (p: PlatformProject) => (mayWrite(p) ? null : "A team's owners and editors change its cloud workspaces.");

/** A project's owner: only they set its inference keys. */
const owns = (p: PlatformProject) => !p.team || p.team.role === "owner";

/** Lines of a textarea, trimmed, without the empty ones. */
const lines = (s: string) =>
  s
    .split(/\r?\n/)
    .map((l) => l.trim())
    .filter(Boolean);

/** The header's Edit and Rebuild, for those who may; else nothing (the
 * sections say why). */
export function WorkspaceActions({ p, w, changed }: { p: PlatformProject; w: Workspace; changed: (w?: Workspace) => void }) {
  const [editing, setEditing] = useState(false);
  const [rebuilding, setRebuilding] = useState(false);
  if (whyNot(p) || w.state === "deleting") return null;
  // Rebuilding makes a box: the beta's, as making one is.
  const rebuilds = session.hosted;
  return (
    <>
      <button type="button" class="secondary" data-hosted-edit onClick={() => setEditing(true)}>
        Edit…
      </button>
      {rebuilds && (
        <button type="button" class="secondary" data-hosted-rebuild-open onClick={() => setRebuilding(true)}>
          Rebuild…
        </button>
      )}
      {editing && <EditSpec p={p} w={w} close={() => setEditing(false)} saved={(v) => (setEditing(false), changed(v))} />}
      {rebuilding && <Rebuild p={p} w={w} close={() => setRebuilding(false)} done={(v) => (setRebuilding(false), changed(v))} />}
    </>
  );
}

/** Edit: what its box is made from next, as `arugula cloud edit`. The
 * finish step stays as it is (the project's settings change it). */
function EditSpec({ p, w, close, saved }: { p: PlatformProject; w: Workspace; close: () => void; saved: (w: Workspace) => void }) {
  const s = w.spec ?? {};
  const [repos, setRepos] = useState((s.repos ?? []).join("\n"));
  const [packages, setPackages] = useState((s.packages ?? []).join("\n"));
  const [setup, setSetup] = useState(s.setup ?? "");
  const [env, setEnv] = useState((s.env ?? []).join("\n"));
  const [size, setSize] = useState<Size>(sizeOf(s));
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState("");
  const submit = async (e: Event) => {
    e.preventDefault();
    setBusy(true);
    setErr("");
    try {
      const spec: Spec = { ...s, repos: lines(repos), packages: lines(packages), setup, env: lines(env).flatMap((l) => l.split(/[\s,]+/)).filter(Boolean), size };
      saved(await call<Workspace>("PUT", `${base(p.id, w.id)}/spec`, spec));
    } catch (e) {
      setErr((e as Error).message);
      setBusy(false);
    }
  };
  return (
    <Modal close={close} class="hosted-edit">
      <form onSubmit={(e) => void submit(e)} data-hosted-edit-form>
        <h2>Edit {w.name}</h2>
        <p class="dim">Its box isn't touched: it wants a rebuild until it's made again from this.</p>
        <label>
          Repos <span class="dim">· HTTPS clone URLs, one a line</span>
          <textarea rows={3} value={repos} placeholder="https://github.com/acme/app" data-hosted-edit-repos onInput={(e) => setRepos(e.currentTarget.value)} />
        </label>
        <label>
          Packages <span class="dim">· one a line</span>
          <textarea rows={2} value={packages} placeholder="ripgrep" data-hosted-edit-packages onInput={(e) => setPackages(e.currentTarget.value)} />
        </label>
        <label>
          Setup script <span class="dim">· runs after both</span>
          <textarea class="sw-mono" rows={5} value={setup} placeholder="npm ci" data-hosted-edit-setup onInput={(e) => setSetup(e.currentTarget.value)} />
        </label>
        <label>
          Env from its secrets <span class="dim">· names, one a line</span>
          <textarea rows={2} value={env} placeholder="NPM_TOKEN" data-hosted-edit-env onInput={(e) => setEnv(e.currentTarget.value)} />
        </label>
        <fieldset class="hosted-new-sizes">
          <legend>Size</legend>
          {SIZES.map((z) => (
            <label key={z.size} class="control-check">
              <input type="radio" name="size" value={z.size} checked={size === z.size} data-hosted-edit-size={z.size} onChange={() => setSize(z.size)} />
              <span>
                {z.label} <span class="dim">· {z.per_hour} an hour while it runs</span>
              </span>
            </label>
          ))}
        </fieldset>
        {err && (
          <p class="control-error" role="alert" data-hosted-edit-error>
            {err}
          </p>
        )}
        <div class="prompt-buttons">
          <button type="button" class="secondary" onClick={close}>
            Cancel
          </button>
          <button type="submit" class="primary" disabled={busy} data-hosted-edit-save>
            {busy ? "Saving…" : "Save"}
          </button>
        </div>
      </form>
    </Modal>
  );
}

/** Rebuild: its box made again from its spec. The CLI asks the box for
 * unpushed work first; this page can't, so it says so. */
function Rebuild({ p, w, close, done }: { p: PlatformProject; w: Workspace; close: () => void; done: (w: Workspace) => void }) {
  const [err, setErr] = useState("");
  return (
    <ConfirmRemove
      title={`Rebuild ${w.name}?`}
      label="Rebuild"
      cancel={close}
      go={() =>
        void call<Workspace>("POST", `${base(p.id, w.id)}/rebuild`).then(done, (e: Error) => setErr(e.message))
      }
    >
      <p>Its box is made again from its spec. Work in it that isn't pushed is lost, and this page can't check for some.</p>
      <p class="dim">From a terminal, the command checks its repos first and stops if there's unpushed work:</p>
      <CopyText text={`arugula cloud rebuild ${w.name} --project ${p.id}`} />
      {err && (
        <p class="control-error" role="alert" data-hosted-rebuild-error>
          {err}
        </p>
      )}
    </ConfirmRemove>
  );
}

/** A port, as `GET …/ports` lists it. */
interface Port {
  port: number;
  label: string;
  public: boolean;
  url?: string;
  public_by?: string;
  public_until?: number;
}

/** Its ports and their public previews, as `arugula cloud preview`. */
export function Previews({ p, w }: { p: PlatformProject; w: Workspace }) {
  const [got, setGot] = useState<{ previews: boolean; ports: Port[] } | null>(null);
  const [err, setErr] = useState("");
  const [port, setPort] = useState("");
  const [tick, setTick] = useState(0);
  const path = `${base(p.id, w.id)}/ports`;
  useEffect(() => {
    let live = true;
    call<{ previews: boolean; ports: Port[] }>("GET", path).then(
      (v) => live && setGot(v),
      (e: Error) => live && setErr(e.message),
    );
    return () => void (live = false);
  }, [path, tick]);
  const why = whyNot(p);
  const act = (f: () => Promise<unknown>) => {
    setErr("");
    f().then(
      () => setTick((t) => t + 1),
      (e: Error) => setErr(e.message),
    );
  };
  const publish = (n: number, renew = false) => act(() => call("PUT", `${path}/${n}/public`, renew ? { renew: true } : {}));
  const off = (n: number) => act(() => call("DELETE", `${path}/${n}/public`));
  const add = (e: Event) => {
    e.preventDefault();
    const n = Number(port);
    if (!Number.isInteger(n) || n < 1 || n > 65535) return setErr("A port is 1 to 65535.");
    setPort("");
    publish(n);
  };
  return (
    <section data-hosted-previews>
      <h2>Public previews</h2>
      <p class="dim">A public port opens at an address of its own, for anyone with it, signed out, for 7 days.</p>
      {got === null && !err && <p class="dim">Loading…</p>}
      {got && !got.previews && <p class="dim" data-hosted-previews-off>Public previews aren't set up on this control.</p>}
      {got && (
        <ul class="hosted-rows">
          {got.ports.map((x) => (
            <li key={x.port} data-hosted-port={x.port}>
              <code>{x.port}</code>
              {x.label ? <span class="dim"> {x.label}</span> : null}{" "}
              {x.public && x.url ? (
                <>
                  <a href={x.url} target="_blank" rel="noopener" data-hosted-port-url>
                    {x.url}
                  </a>
                  <span class="dim">
                    {x.public_by ? ` · by ${nameIn(p)(x.public_by) ?? "someone outside it"}` : ""}
                    {x.public_until ? (
                      <>
                        {" "}
                        · ends <Time at={x.public_until} long />
                      </>
                    ) : null}
                  </span>
                </>
              ) : (
                <span class="dim">private</span>
              )}
              {!why && got.previews && (
                <span class="hosted-row-actions">
                  {x.public ? (
                    <>
                      <button type="button" class="secondary small" data-hosted-port-renew onClick={() => publish(x.port, true)}>
                        Renew
                      </button>
                      <button type="button" class="secondary danger small" data-hosted-port-off onClick={() => off(x.port)}>
                        Turn off
                      </button>
                    </>
                  ) : (
                    <button type="button" class="secondary small" data-hosted-port-public onClick={() => publish(x.port)}>
                      Make public
                    </button>
                  )}
                </span>
              )}
            </li>
          ))}
        </ul>
      )}
      {got?.previews && !why && (
        <form class="hosted-add" onSubmit={add}>
          <input inputMode="numeric" aria-label="Port" placeholder="5173" value={port} data-hosted-port-new onInput={(e) => setPort(e.currentTarget.value)} />
          <button type="submit" class="secondary">
            Make public
          </button>
        </form>
      )}
      {got?.previews && why && <p class="dim">{why}</p>}
      {err && (
        <p class="control-error" role="alert">
          {err}
        </p>
      )}
    </section>
  );
}

/** A secret's name: capital letters, digits and `_` (`secrets::valid_name`). */
const SECRET = /^[A-Z0-9_]{1,128}$/;

/** Its secrets, by name (never a value), as `arugula cloud secret`. */
export function Secrets({ p, w }: { p: PlatformProject; w: Workspace }) {
  const [names, setNames] = useState<string[] | null>(null);
  const [err, setErr] = useState("");
  const [name, setName] = useState("");
  const [value, setValue] = useState("");
  const [removing, setRemoving] = useState<string | null>(null);
  const [tick, setTick] = useState(0);
  const path = `${base(p.id, w.id)}/secrets`;
  useEffect(() => {
    let live = true;
    call<{ secrets: { name: string }[] }>("GET", path).then(
      (v) => live && setNames(v.secrets.map((s) => s.name)),
      (e: Error) => live && setErr(e.message),
    );
    return () => void (live = false);
  }, [path, tick]);
  const why = whyNot(p);
  const set = (e: Event) => {
    e.preventDefault();
    const n = name.trim();
    if (!SECRET.test(n)) return setErr("A secret's name: capital letters, digits and '_'.");
    if (!value) return setErr("A secret needs a value.");
    setErr("");
    call("PUT", `${path}/${encodeURIComponent(n)}`, { value }).then(
      () => (setName(""), setValue(""), setTick((t) => t + 1)),
      (e: Error) => setErr(e.message),
    );
  };
  const wanted = (w.spec?.env ?? []).filter((n) => names && !names.includes(n));
  return (
    <section data-hosted-secrets>
      <h2>Secrets</h2>
      <p class="dim">
        Its box reads them on its next setup: the env its spec names, and <code>GIT_TOKEN</code> (with <code>GIT_USERNAME</code>) to clone what the
        GitHub App doesn't. Never shown again.
      </p>
      {names === null && !err && <p class="dim">Loading…</p>}
      {names && !names.length && <p class="dim">None yet.</p>}
      {names && names.length > 0 && (
        <ul class="hosted-rows">
          {names.map((n) => (
            <li key={n} data-hosted-secret={n}>
              <code>{n}</code>
              {!why && (
                <span class="hosted-row-actions">
                  <button type="button" class="secondary danger small" data-hosted-secret-remove={n} onClick={() => setRemoving(n)}>
                    Remove
                  </button>
                </span>
              )}
            </li>
          ))}
        </ul>
      )}
      {wanted.length > 0 && <p class="dim">Its spec names {wanted.join(", ")}, not set yet.</p>}
      {why ? (
        <p class="dim">{why}</p>
      ) : (
        <form class="hosted-add" onSubmit={set} data-hosted-secret-form>
          <input aria-label="Name" placeholder="NPM_TOKEN" value={name} data-hosted-secret-name onInput={(e) => setName(e.currentTarget.value.toUpperCase())} />
          <input type="password" autocomplete="off" aria-label="Value" placeholder="value" value={value} data-hosted-secret-value onInput={(e) => setValue(e.currentTarget.value)} />
          <button type="submit" class="secondary" data-hosted-secret-set>
            Set
          </button>
        </form>
      )}
      {err && (
        <p class="control-error" role="alert">
          {err}
        </p>
      )}
      {removing && (
        <ConfirmRemove
          title={`Remove ${removing}?`}
          cancel={() => setRemoving(null)}
          go={() => {
            const n = removing;
            setRemoving(null);
            call("DELETE", `${path}/${encodeURIComponent(n)}`).then(
              () => setTick((t) => t + 1),
              (e: Error) => setErr(e.message),
            );
          }}
        >
          <p>Its box keeps it until its next setup.</p>
        </ConfirmRemove>
      )}
    </section>
  );
}

/** A project's key, as `GET …/inference-keys` lists it. */
interface Key {
  provider: string;
  set: boolean;
  set_by?: string | null;
  updated_at?: number | null;
}

const PROVIDER: Record<string, string> = { anthropic: "Anthropic", openai: "OpenAI" };

/** The project's own inference keys, which its hosted agents run on, as
 * `arugula cloud key`: set or not, never read back. */
export function InferenceKeys({ p }: { p: PlatformProject }) {
  const [keys, setKeys] = useState<Key[] | null>(null);
  const [err, setErr] = useState("");
  const [setting, setSetting] = useState<string | null>(null);
  const [value, setValue] = useState("");
  const path = `/api/projects/${encodeURIComponent(p.id)}/inference-keys`;
  const load = () =>
    call<{ keys: Key[] }>("GET", path).then(
      (v) => setKeys(v.keys),
      (e: Error) => setErr(e.message),
    );
  useEffect(() => void load(), [path]);
  const mine = owns(p);
  const save = (e: Event) => {
    e.preventDefault();
    if (!setting) return;
    setErr("");
    call("PUT", `${path}/${setting}`, { value: value.trim() }).then(
      () => (setSetting(null), setValue(""), void load()),
      (e: Error) => setErr(e.message),
    );
  };
  const clear = (provider: string) => {
    setErr("");
    call("DELETE", `${path}/${provider}`).then(() => void load(), (e: Error) => setErr(e.message));
  };
  return (
    <section data-hosted-keys>
      <h2>Inference keys</h2>
      <p class="dim">The project's, for every one of its cloud workspaces: its hosted agents run on them. Kept sealed, never in a box, never shown again.</p>
      {keys === null && !err && <p class="dim">Loading…</p>}
      {keys && (
        <ul class="hosted-rows">
          {keys.map((k) => (
            <li key={k.provider} data-hosted-key={k.provider} data-set={k.set ? "yes" : "no"}>
              {PROVIDER[k.provider] ?? k.provider}{" "}
              <span class="dim">
                {k.set ? `set${k.set_by ? ` by ${nameIn(p)(k.set_by) ?? "a former owner"}` : ""}` : "not set"}
                {k.set && k.updated_at ? (
                  <>
                    {" "}
                    <Time at={k.updated_at} />
                  </>
                ) : null}
              </span>
              {mine && setting !== k.provider && (
                <span class="hosted-row-actions">
                  <button type="button" class="secondary small" data-hosted-key-set={k.provider} onClick={() => (setSetting(k.provider), setValue(""))}>
                    {k.set ? "Replace" : "Set"}
                  </button>
                  {k.set && (
                    <button type="button" class="secondary danger small" data-hosted-key-clear={k.provider} onClick={() => clear(k.provider)}>
                      Clear
                    </button>
                  )}
                </span>
              )}
              {setting === k.provider && (
                <form class="hosted-add" onSubmit={save}>
                  <input
                    type="password"
                    autocomplete="off"
                    autofocus
                    aria-label={`${PROVIDER[k.provider] ?? k.provider} key`}
                    placeholder={k.provider === "anthropic" ? "An API key, or a token from claude setup-token" : "An API key"}
                    value={value}
                    data-hosted-key-value
                    onInput={(e) => setValue(e.currentTarget.value)}
                  />
                  <button type="submit" class="primary small">
                    Save
                  </button>
                  <button type="button" class="secondary small" onClick={() => setSetting(null)}>
                    Cancel
                  </button>
                </form>
              )}
            </li>
          ))}
        </ul>
      )}
      {!mine && <p class="dim">Only a project's owners set its keys.</p>}
      {err && (
        <p class="control-error" role="alert">
          {err}
        </p>
      )}
    </section>
  );
}
