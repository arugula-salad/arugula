// Cloud workspace blocks (#1117): one of the machine owner's Cloud
// workspaces beside a pane, put there by an agent (`show` kind
// `cloud_workspace`). Control's page can't be framed, so this draws the
// workspace with control mode's own views (ui/cloud-workspaces.tsx): its
// status and tabs, a tab's transcript with its approvals and questions, the
// prompt box, Interrupt and Changes. The daemon reads the workspace and its
// tabs from control while the block is drawn (the block's state); the views'
// other reads and writes go through the block's `cloud` method, which the
// daemon sends to control as the machine, for this workspace only. On
// control's page, where the person's own Cloud workspaces are, Open shows
// it there; elsewhere it's a link to control's page.

import { render } from "preact";
import { useMemo, useState } from "preact/hooks";
import type { Client } from "../client";
import { CloudApiContext, cloudTabAttention, cloudWorkspaces, cloudWorkspaceState, type CloudApi, type CloudTab, type CloudWorkspace } from "../../shared/cloud";
import type { PaneId } from "../../shared/proto";
import { CloudWorkspaceChanges } from "../../platform/cloud/workspace-changes";
import { CloudSetupLog, CloudTabView } from "../../platform/cloud/workspaces";
import { registerBlock, type BlockView } from "./view";

interface CloudWorkspaceBlockState {
  cloud_workspace: string;
  tab: string | null;
  workspace: CloudWorkspace | null;
  tabs: CloudTab[];
  link: string | null;
  error: string | null;
  loading: boolean;
}

/** Control's `/api/cloud/` through the block. */
function blockApi(client: Client, id: PaneId): CloudApi {
  return async <T,>(path: string, method = "GET", body?: unknown): Promise<T> => {
    const res = await client.request("POST", `/api/blocks/${id}/call/cloud`, { method, path, ...(body === undefined ? {} : { body }) });
    const value = await res.json<T & { error?: string }>().catch(() => ({}) as T & { error?: string });
    if (!res.ok) throw new Error(value.error ?? `Request failed (${res.status})`);
    return value;
  };
}

const tabName = (t: CloudTab) => `${t.agent_name} · ${t.runtime}`;

/** Open: on control's page, its own view of the workspace when it's this
 * person's; elsewhere control's page in a new tab. */
function OpenInControl({ s, tab }: { s: CloudWorkspaceBlockState; tab: string | null }) {
  if (cloudWorkspaces.find(s.cloud_workspace)) {
    return <button data-cloud-ws-open title="Show this Cloud workspace in Cloud workspaces" onClick={() => cloudWorkspaces.open(s.cloud_workspace, tab ?? undefined)}>Open</button>;
  }
  if (!s.link) return null;
  const href = tab ? s.link.replace(/#cloud-ws=.*$/, `#cloud-ws=${s.cloud_workspace}/${tab}`) : s.link;
  return <a class="browser-open" data-cloud-ws-link href={href} target="_blank" rel="noopener noreferrer" title="Open this Cloud workspace on Arugula control's page">Open in control</a>;
}

function Overview({ w, tabs, may, pick }: { w: CloudWorkspace; tabs: CloudTab[]; may: boolean; pick: (tab: string) => void }) {
  const { word } = cloudWorkspaceState(w);
  const spec = w.saved_spec;
  return (
    <div class="cloud-ws-home" data-cloud-ws-block-home>
      <p class="dim" data-cloud-home-status>
        {word}
        {w.status === "provisioning" && ": installing packages, cloning repositories and running the setup script."}
        {w.status === "ready" && w.sandbox_status === "suspended" && ": its machine sleeps until a tab gets a prompt."}
      </p>
      {w.status === "failed" && w.error && <pre class="cloud-ws-error" role="alert" data-cloud-error>{w.error}</pre>}
      <h3>Repositories</h3>
      {spec.repositories.length ? (
        <ul class="cloud-ws-repos">{spec.repositories.map((r) => <li key={r.mount ?? r.url}><span class="mono">{r.mount}</span> · {r.url}{r.ref ? ` @ ${r.ref}` : ""}</li>)}</ul>
      ) : (
        <p class="dim">None: tabs start in /home/sprite.</p>
      )}
      <h3>Tabs</h3>
      {tabs.length ? (
        <ul class="cloud-ws-tabs">{tabs.map((t) => <li key={t.id}><button onClick={() => pick(t.id)}>{tabName(t)}</button> <span class="dim">{t.status.replaceAll("_", " ")} · {t.cwd}</span></li>)}</ul>
      ) : (
        <p class="dim">No tabs yet.</p>
      )}
      {may ? (
        <>
          <h3>Changes</h3>
          <CloudWorkspaceChanges w={w} tabs={tabs} />
          {w.status === "failed" && <CloudSetupLog w={w} />}
        </>
      ) : (
        <p class="dim">You're watching this session: its tabs' transcripts and changes are for the people who can change it.</p>
      )}
    </div>
  );
}

function CloudWorkspaceBlock({ client, id, s }: { client: Client; id: PaneId; s: CloudWorkspaceBlockState | null }) {
  const api = useMemo(() => blockApi(client, id), [client, id]);
  const [picked, setPicked] = useState<string | null | undefined>(undefined);
  if (!s) return <div class="review"><p class="dim">Loading…</p></div>;
  const w = s.workspace;
  const tabs = s.tabs ?? [];
  // The tab it was opened on, until another is picked.
  const tabId = picked === undefined ? s.tab : picked;
  const tab = tabId ? tabs.find((t) => t.id === tabId) : undefined;
  const may = client.role(client.sessionOfPane(id)) !== "viewer";
  const state = w ? cloudWorkspaceState(w) : null;
  return (
    <div class="review cloud-ws-block" data-cloud-ws-block={id} data-cloud-workspace={s.cloud_workspace} data-cloud-state={state?.word}>
      <div class="review-bar">
        <span class="review-path">
          <b>{w?.name ?? s.cloud_workspace}</b> <span class="dim">Cloud workspace{state ? ` · ${state.word}` : ""}</span>
        </span>
        <OpenInControl s={s} tab={tabId} />
      </div>
      {w && (
        <div class="tabstrip cloud-tabstrip">
          <div class="tabbar" role="tablist" aria-label={`Tabs in ${w.name}`}>
            <div class={tabId ? "tab" : "tab selected"} role="tab" aria-selected={!tabId} tabIndex={tabId ? -1 : 0} data-cloud-ws-overview onClick={() => setPicked(null)}>
              <span class="tab-label">Overview</span>
            </div>
            {tabs.map((t) => {
              const selected = tabId === t.id;
              const attention = cloudTabAttention(t);
              return (
                <div key={t.id} class={selected ? "tab selected" : "tab"} role="tab" aria-selected={selected} tabIndex={selected ? 0 : -1} data-cloud-tab={t.id} data-attention={attention} title={`${t.agent_name} on ${t.runtime} (${t.model}) in ${t.cwd}`} onClick={() => setPicked(t.id)}>
                  <span class="host-tag" title={t.model}>{t.runtime}</span>
                  <span class="tab-label">{t.agent_name}</span>
                  {attention === "needs_input" ? <span class="attention" title="Needs you">●</span> : attention === "working" ? <span class="attention working" title="Working">●</span> : null}
                </div>
              );
            })}
          </div>
        </div>
      )}
      <div class="review-body cloud-ws-view">
        {s.error && <p role="alert" class="control-error">{s.error}</p>}
        {!w ? (
          <p class="dim cloud-ws-block-note">{s.loading ? "Reading it from Arugula control…" : "Arugula control didn't say what it is."}</p>
        ) : tabId && !may ? (
          <p class="dim cloud-ws-block-note">You're watching this session: a tab's transcript is for the people who can change it.</p>
        ) : tabId ? (
          <CloudApiContext.Provider value={api}>
            <CloudTabView key={tabId} w={w} tab={tab} id={tabId} tabs={tabs} />
          </CloudApiContext.Provider>
        ) : (
          <CloudApiContext.Provider value={api}>
            <Overview w={w} tabs={tabs} may={may} pick={setPicked} />
          </CloudApiContext.Provider>
        )}
      </div>
    </div>
  );
}

function plain(s: CloudWorkspaceBlockState | null): string {
  if (!s) return "";
  const w = s.workspace;
  const lines = [w ? `${w.name} (${s.cloud_workspace}): ${cloudWorkspaceState(w).word}` : s.cloud_workspace];
  for (const t of s.tabs ?? []) lines.push(`${t.id} ${tabName(t)}: ${t.status}`);
  if (s.error) lines.push(s.error);
  return lines.join("\n");
}

registerBlock("cloud_workspace", (client, id): BlockView => {
  const host = document.createElement("div");
  host.className = "block block-cloud-workspace";
  let state: CloudWorkspaceBlockState | null = null;
  const draw = () => render(<CloudWorkspaceBlock client={client} id={id} s={state} />, host);
  draw();
  const off = client.subscribe(draw);
  // On control's page, Open depends on the person's own Cloud workspaces.
  const offCloud = cloudWorkspaces.subscribe(draw);
  return {
    host,
    setVisible: (v) => (host.style.display = v ? "" : "none"),
    update: (s) => {
      state = s as CloudWorkspaceBlockState;
      draw();
    },
    title: () => state?.workspace?.name ?? "Cloud workspace",
    text: () => plain(state),
    focus: () => host.querySelector<HTMLElement>("button, [role=tab]")?.focus(),
    dispose: () => {
      off();
      offCloud();
      render(null, host);
      host.remove();
    },
  };
});
