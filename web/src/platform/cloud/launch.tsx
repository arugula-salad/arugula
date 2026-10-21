// New agent → Run on: Cloud (S40, M119, #1076): an agent opens as a tab in
// a Cloud workspace, picked from the account's, or made here from the one
// repository it's given. Cloud runs (a fresh checkout per agent) are retired.

import type { ComponentChildren } from "preact";
import { useEffect, useRef, useState } from "preact/hooks";
import {
  cloudWorkspaceMounts, cloudWorkspaceRoute, cloudWorkspaces, cloudWorkspaceState, useCloudPanes,
  type CloudAgent, type CloudModel, type CloudTab, type CloudWorkspace, type CloudWorkspaceSpec,
} from "../../shared/cloud";
import { useCloudApi } from "../../shared/platform-api";

/** Where the agent goes: a Cloud workspace's id, or a new one. */
const NEW = "new";

/** What a Cloud workspace made from `url` is called: its repository's name. */
export function cloudWorkspaceName(url: string): string {
  try {
    const name = new URL(url).pathname.split("/").filter(Boolean).pop()?.replace(/\.git$/, "");
    if (name) return name.slice(0, 120);
  } catch { /* Not a URL yet: control says so. */ }
  return "Cloud workspace";
}

/** Cloud workspaces a tab can open in: not going, and not failed with no
 * machine. */
const usable = (w: CloudWorkspace) => w.status !== "deleting" && w.status !== "failed";

export function CloudAgentLaunch({ close, choice }: { close: () => void; choice: ComponentChildren }) {
  const api = useCloudApi();
  useCloudPanes();
  const list = cloudWorkspaces.list.filter(usable);
  // Until one is picked: the shown Cloud workspace, else the newest, else
  // a new one (following the list as it loads).
  const [chosen, setWhere] = useState<string | null>(null);
  const shown = cloudWorkspaces.find(cloudWorkspaceRoute()?.workspace ?? null);
  const where = chosen ?? (shown && usable(shown) ? shown.id : list[0]?.id ?? NEW);
  const [repository, setRepository] = useState("");
  const [gitRef, setGitRef] = useState("");
  const [secret, setSecret] = useState("");
  const [names, setNames] = useState<string[]>([]);
  const [agents, setAgents] = useState<CloudAgent[]>([]);
  const [models, setModels] = useState<CloudModel[]>([]);
  const [agent, setAgent] = useState("curated_coding");
  const [model, setModel] = useState(0);
  const [cwd, setCwd] = useState("");
  const [prompt, setPrompt] = useState("");
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  // A retry after a lost response makes neither twice: the same keys while
  // nothing changes.
  const workspaceKey = useRef(crypto.randomUUID());
  const tabKey = useRef(crypto.randomUUID());
  useEffect(() => { workspaceKey.current = crypto.randomUUID(); }, [repository, gitRef, secret]);
  useEffect(() => { tabKey.current = crypto.randomUUID(); }, [where, repository, gitRef, secret, agent, model, cwd, prompt]);
  useEffect(() => {
    void api<{ agents: CloudAgent[] }>("agents").then((v) => { setAgents(v.agents); if (!v.agents.some((a) => a.id === agent) && v.agents[0]) setAgent(v.agents[0].id); }, (e: Error) => setError(e.message));
    void api<{ models: CloudModel[] }>("models").then((v) => setModels(v.models), (e: Error) => setError(e.message));
    void api<{ names: string[] }>("secrets").then((v) => setNames(v.names), () => {});
  }, []);
  const picked = where === NEW ? undefined : cloudWorkspaces.find(where);
  const mounts = picked ? cloudWorkspaceMounts(picked) : [];
  useEffect(() => setCwd(mounts[0] ?? ""), [where]);

  const submit = async (e: Event) => {
    e.preventDefault(); setBusy(true); setError("");
    const pick = models[model];
    try {
      let w = picked;
      if (!w) {
        const spec: CloudWorkspaceSpec = {
          name: cloudWorkspaceName(repository.trim()),
          repositories: [{ url: repository.trim(), ...(gitRef.trim() ? { ref: gitRef.trim() } : {}), ...(secret ? { secret } : {}) }],
          packages: {},
        };
        w = await api<CloudWorkspace>("workspaces", "POST", { request_key: workspaceKey.current, spec });
      }
      const t = await api<CloudTab>(`workspaces/${w.id}/tabs`, "POST", {
        request_key: tabKey.current, agent_id: agent, runtime: pick.runtime, model: pick.model,
        ...(cwd ? { cwd } : {}), ...(prompt.trim() ? { prompt } : {}),
      });
      close();
      cloudWorkspaces.refresh();
      cloudWorkspaces.open(w.id, t.id);
    } catch (e) { setError((e as Error).message); } finally { setBusy(false); }
  };

  return (
    <div class="prompt-backdrop" role="dialog" aria-label="Start an agent" onKeyDown={(e) => e.key === "Escape" && close()}>
      <form class="prompt agent-dialog cloud-launch" aria-label="Start an agent" onSubmit={submit}>
        <h2>Start an agent</h2>
        {choice}
        <p class="dim">It opens as a tab in a Cloud workspace: a machine in the cloud with your repositories on it, which its tabs share.</p>
        <label>Cloud workspace<select data-cloud-launch-where value={where} onChange={(e) => setWhere(e.currentTarget.value)}>
          {list.map((w) => <option key={w.id} value={w.id}>{w.name} · {cloudWorkspaceState(w).word}</option>)}
          <option value={NEW}>New, from a repository…</option>
        </select></label>
        {where === NEW && <>
          <label>Repository URL<input type="url" required placeholder="https://github.com/owner/repository" value={repository} onInput={(e) => setRepository(e.currentTarget.value)} /></label>
          <label>Branch or tag (optional)<input value={gitRef} onInput={(e) => setGitRef(e.currentTarget.value)} /></label>
          <label>Repository credential<select value={secret} onChange={(e) => setSecret(e.currentTarget.value)}><option value="">Public repository</option>{names.map((n) => <option key={n}>{n}</option>)}</select></label>
          <p class="dim">Packages, a setup script and more repositories are set in <b>New Cloud workspace</b>.</p>
        </>}
        <label>Agent<select value={agent} onChange={(e) => setAgent(e.currentTarget.value)}>{agents.map((a) => <option key={a.id} value={a.id}>{a.definition.name}{a.curated ? " · Arugula" : ""}</option>)}</select></label>
        <label>Runtime and model<select value={model} onChange={(e) => setModel(Number(e.currentTarget.value))}>{models.map((m, i) => <option key={`${m.runtime}:${m.model}`} value={i}>{m.runtime} · {m.model}</option>)}</select></label>
        {mounts.length > 1 && <label>Starts in<select value={cwd} onChange={(e) => setCwd(e.currentTarget.value)}>{mounts.map((m) => <option key={m}>{m}</option>)}</select></label>}
        <label>Prompt<textarea rows={4} value={prompt} placeholder="What should it do?" onInput={(e) => setPrompt(e.currentTarget.value)} /></label>
        {(where === NEW || picked?.status === "provisioning") && <p class="dim">The prompt waits until the Cloud workspace is set up.</p>}
        {error && <p role="alert" class="control-error">{error}</p>}
        <div class="prompt-buttons">
          <button type="button" onClick={close}>Cancel</button>
          <button type="submit" class="primary" disabled={busy || !agents.length || !models.length}>{busy ? "Opening…" : "Start"}</button>
        </div>
      </form>
    </div>
  );
}
