// A Cloud workspace's Settings (S40, M117, #1074): what its machine is built
// from, as saved, against what it was built with. Saving PATCHes control's
// `/api/cloud/workspaces/{id}`: the build fields reach the machine on its
// next rebuild, secrets reach it at once. `pending` (Managoat's
// `changed_fields`, with values from the specs) is what a rebuild changes.

import { useEffect, useState } from "preact/hooks";
import { CloudWorkspaceRebuildNote, RebuildCloudWorkspace } from "./cloud-workspace-rebuild";
import { cloudApi as api, cloudWorkspaceRebuildBadge, cloudWorkspaces, type CloudWorkspace, type CloudWorkspaceChange, type CloudWorkspaceSpec } from "../cloud";

/** Words split on commas and spaces: "ffmpeg, jq" → ["ffmpeg", "jq"]. */
const words = (s: string) => s.split(/[\s,]+/).filter(Boolean);

const FIELD_NAMES: Record<string, string> = {
  repositories: "Repositories", packages: "Packages", setup_script: "Setup script", setup_timeout_seconds: "Setup timeout",
  env_vars: "Environment variables", networking: "Network",
};

/** One side of a change, as words. */
function shown(field: string, value: unknown): string {
  if (value === null || value === undefined) {
    if (field === "setup_timeout_seconds") return "120 s (the default)";
    if (field === "networking") return "unrestricted";
    return "not known";
  }
  switch (field) {
    case "repositories": {
      const list = value as CloudWorkspaceSpec["repositories"];
      return list.length ? list.map((r) => `${r.mount} ← ${r.url}${r.ref ? ` @ ${r.ref}` : ""}`).join("\n") : "none";
    }
    case "packages": {
      const p = value as CloudWorkspaceSpec["packages"];
      const all = [...(p.apt ?? []).map((n) => `apt ${n}`), ...(p.npm ?? []).map((n) => `npm ${n}`)];
      return all.length ? all.join(", ") : "none";
    }
    case "setup_script": return (value as string) || "none";
    case "setup_timeout_seconds": return `${value as number} s`;
    // Names only: Managoat never says a value, and nor does control.
    case "env_vars": return (value as string[]).length ? (value as string[]).join(", ") : "none";
    case "networking": {
      const n = value as NonNullable<CloudWorkspaceSpec["networking"]>;
      return n.type === "limited" ? `limited to ${(n.allowed_hosts ?? []).join(", ") || "no hosts"}` : "unrestricted";
    }
    default: return JSON.stringify(value);
  }
}

function Change({ c }: { c: CloudWorkspaceChange }) {
  const before = shown(c.field, c.before), after = shown(c.field, c.after);
  return (
    <tr data-pending-field={c.field}>
      <th scope="row">{FIELD_NAMES[c.field] ?? c.field}</th>
      <td><pre>{before}</pre></td>
      <td><pre>{before === after && c.field === "env_vars" ? `${after} (a value changed)` : after}</pre></td>
    </tr>
  );
}

/** The workspace's Settings: what a rebuild changes, the last setup error,
 * and the form over the saved spec. */
export function CloudWorkspaceSettings({ w }: { w: CloudWorkspace }) {
  const [editing, setEditing] = useState(false);
  const badge = cloudWorkspaceRebuildBadge(w);
  const pending = w.pending ?? [];
  return (
    <section class="cloud-ws-settings" data-cloud-settings>
      <div class="cloud-ws-settings-head">
        <h3>Settings</h3>
        {!editing && w.status !== "deleting" && <button data-edit-cloud-settings onClick={() => setEditing(true)}>Edit settings</button>}
        {!editing && <RebuildCloudWorkspace w={w} place="settings" />}
      </div>
      {w.status !== "failed" && <CloudWorkspaceRebuildNote w={w} />}
      {w.settings_applied === false && <p class="dim" role="status" data-settings-unsent>Saved. Sending them to Managoat; control keeps trying.</p>}
      {badge ? (
        <>
          <p data-cloud-rebuild-badge><b>{badge}.</b> The machine was built before these were saved; they reach it when it's rebuilt. Secrets reach it at once.</p>
          {pending.length > 0 && (
            <table class="cloud-ws-pending" data-cloud-pending>
              <thead><tr><th scope="col">Setting</th><th scope="col">Built</th><th scope="col">Saved</th></tr></thead>
              <tbody>{pending.map((c) => <Change key={c.field} c={c} />)}</tbody>
            </table>
          )}
        </>
      ) : (
        w.status === "ready" && <p class="dim">The machine is built from these settings.</p>
      )}
      {w.error && w.status !== "failed" && (
        <details class="cloud-ws-last-error">
          <summary>Last setup error</summary>
          <pre class="cloud-ws-error">{w.error}</pre>
        </details>
      )}
      {editing && <CloudWorkspaceSettingsForm w={w} close={() => setEditing(false)} />}
    </section>
  );
}

type RepositoryDraft = { url: string; ref: string; mount: string; secret: string };

function CloudWorkspaceSettingsForm({ w, close }: { w: CloudWorkspace; close: () => void }) {
  // A copy of the saved spec as it was when the form opened: the polls
  // don't move what's being typed.
  const [spec] = useState(() => w.saved_spec);
  const [name, setName] = useState(spec.name);
  const [repositories, setRepositories] = useState<RepositoryDraft[]>(() =>
    spec.repositories.map((r) => ({ url: r.url, ref: r.ref ?? "", mount: r.mount ?? "", secret: r.secret ?? "" })));
  const [apt, setApt] = useState((spec.packages.apt ?? []).join(" "));
  const [npm, setNpm] = useState((spec.packages.npm ?? []).join(" "));
  const [script, setScript] = useState(spec.setup_script ?? "");
  const [timeout, setTimeoutSeconds] = useState(spec.setup_timeout_seconds ? String(spec.setup_timeout_seconds) : "");
  const [env, setEnv] = useState(Object.entries(spec.env_vars ?? {}).map(([k, v]) => `${k}=${v}`).join("\n"));
  const [network, setNetwork] = useState<"unrestricted" | "limited">(spec.networking?.type ?? "unrestricted");
  const [hosts, setHosts] = useState((spec.networking?.allowed_hosts ?? []).join(" "));
  const [secrets, setSecrets] = useState<string[]>(spec.secrets ?? []);
  const [names, setNames] = useState<string[]>([]);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  useEffect(() => { void api<{ names: string[] }>("secrets").then((v) => setNames(v.names), () => {}); }, []);
  const choices = [...new Set([...names, ...secrets, ...repositories.map((r) => r.secret).filter(Boolean)])].sort();
  const setRepository = (i: number, change: Partial<RepositoryDraft>) => setRepositories(repositories.map((r, j) => (j === i ? { ...r, ...change } : r)));
  const submit = async (e: Event) => {
    e.preventDefault(); setError("");
    const env_vars: Record<string, string> = {};
    for (const line of env.split("\n").map((l) => l.trim()).filter(Boolean)) {
      const at = line.indexOf("=");
      if (at < 1) { setError(`"${line}" isn't NAME=value.`); return; }
      env_vars[line.slice(0, at).trim()] = line.slice(at + 1);
    }
    const next: CloudWorkspaceSpec = {
      name: name.trim(),
      repositories: repositories.filter((r) => r.url.trim()).map((r) => ({
        url: r.url.trim(), ...(r.ref.trim() ? { ref: r.ref.trim() } : {}), ...(r.mount.trim() ? { mount: r.mount.trim() } : {}), ...(r.secret ? { secret: r.secret } : {}),
      })),
      packages: { ...(words(apt).length ? { apt: words(apt) } : {}), ...(words(npm).length ? { npm: words(npm) } : {}) },
      setup_script: script,
      ...(timeout.trim() ? { setup_timeout_seconds: Number(timeout) } : {}),
      env_vars,
      ...(network === "limited" ? { networking: { type: "limited" as const, allowed_hosts: words(hosts) } } : {}),
      secrets,
    };
    setBusy(true);
    try {
      await api<CloudWorkspace>(`workspaces/${w.id}`, "PATCH", { spec: next });
      close();
      cloudWorkspaces.refresh();
    } catch (e) { setError((e as Error).message); } finally { setBusy(false); }
  };
  return (
    <form class="cloud-form cloud-ws-settings-form" aria-label="Cloud workspace settings" onSubmit={submit}>
      <p class="dim">Repositories, packages, the setup script, its timeout, environment variables and the network reach the machine when it's rebuilt. Secrets reach it as soon as you save.</p>
      <label>Name<input required maxLength={120} value={name} onInput={(e) => setName(e.currentTarget.value)} /></label>
      <fieldset class="cloud-ws-repositories">
        <legend>Repositories</legend>
        {repositories.map((r, i) => (
          <div key={i} class="cloud-ws-repository" data-repository={i}>
            <label>Repository URL<input type="url" placeholder="https://github.com/owner/repository" value={r.url} onInput={(e) => setRepository(i, { url: e.currentTarget.value })} /></label>
            <label>Branch or tag (optional)<input value={r.ref} onInput={(e) => setRepository(i, { ref: e.currentTarget.value })} /></label>
            <label>Mount (optional)<input placeholder="/home/sprite/repository" value={r.mount} onInput={(e) => setRepository(i, { mount: e.currentTarget.value })} /></label>
            <label>Repository credential<select value={r.secret} onChange={(e) => setRepository(i, { secret: e.currentTarget.value })}><option value="">Public repository</option>{choices.map((n) => <option key={n}>{n}</option>)}</select></label>
            <button type="button" onClick={() => setRepositories(repositories.filter((_, j) => j !== i))}>Remove repository</button>
          </div>
        ))}
        {repositories.length < 10 && <button type="button" onClick={() => setRepositories([...repositories, { url: "", ref: "", mount: "", secret: "" }])}>Add a repository</button>}
      </fieldset>
      <label>apt packages<input placeholder="ffmpeg jq" value={apt} onInput={(e) => setApt(e.currentTarget.value)} /></label>
      <label>npm packages<input placeholder="pnpm typescript" value={npm} onInput={(e) => setNpm(e.currentTarget.value)} /></label>
      <label>Setup script<textarea rows={4} value={script} onInput={(e) => setScript(e.currentTarget.value)} /></label>
      <label>Setup timeout in seconds (optional)<input type="number" min={1} max={900} placeholder="120" value={timeout} onInput={(e) => setTimeoutSeconds(e.currentTarget.value)} /></label>
      <label>Environment variables, one NAME=value a line<textarea rows={3} class="mono" value={env} onInput={(e) => setEnv(e.currentTarget.value)} /></label>
      <label>Network<select value={network} onChange={(e) => setNetwork(e.currentTarget.value as "unrestricted" | "limited")}><option value="unrestricted">Unrestricted</option><option value="limited">Only some hosts</option></select></label>
      {network === "limited" && <label>Allowed hosts<input placeholder="github.com registry.npmjs.org" value={hosts} onInput={(e) => setHosts(e.currentTarget.value)} /></label>}
      {choices.length > 0 && (
        <fieldset>
          <legend>Secrets the agents may use</legend>
          {choices.map((n) => <label key={n}><input type="checkbox" checked={secrets.includes(n)} onChange={(e) => setSecrets(e.currentTarget.checked ? [...secrets, n] : secrets.filter((v) => v !== n))} />{n}</label>)}
        </fieldset>
      )}
      {error && <p role="alert" class="control-error">{error}</p>}
      <div class="prompt-buttons">
        <button type="button" onClick={close}>Cancel</button>
        <button class="primary" disabled={busy}>{busy ? "Saving…" : "Save settings"}</button>
      </div>
    </form>
  );
}
