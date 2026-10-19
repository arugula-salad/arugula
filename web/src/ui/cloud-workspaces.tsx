// Cloud workspaces in the shell (S40, M116, #1073): a session backed by a
// Managoat workspace, whose tabs are agent conversations on one machine.
// The sidebar's group, the tab strip, the header's status and Delete, the
// main view (the list, a workspace, or a tab's transcript) and the dialogs
// that make a workspace and open a tab. Everything comes from control's
// `/api/cloud/workspaces`, through the `cloudWorkspaces` store.

import { useEffect, useRef, useState } from "preact/hooks";
import {
  cloudApi as api, cloudPanes, cloudTabAttention, cloudWorkspaceMounts, cloudWorkspaceRoute, cloudWorkspaces, cloudWorkspaceState, setupStepName, useCloudPanes,
  type CloudAgent, type CloudModel, type CloudSetupLog, type CloudTab, type CloudWorkspace, type CloudWorkspaceSpec, type PastCloudTab,
} from "../cloud";
import { ConfirmRemove } from "./confirm";
import { CloudTranscript, useCloudConversation } from "./cloud";
import { CloudWorkspaceSettings } from "./cloud-workspace-settings";
import { CloudWorkspaceChanges } from "./cloud-workspace-changes";
import { RetryCloudWorkspace } from "./cloud-workspace-rebuild";

/** Re-render on the store and the route; returns the route. */
export function useCloudWorkspaces() {
  useCloudPanes();
  return cloudWorkspaceRoute();
}

const tabsOf = (w: CloudWorkspace) => cloudWorkspaces.tabs.get(w.id) ?? [];
/** Tabs not yet closing: the ones a delete ends. */
const liveTabs = (w: CloudWorkspace) => tabsOf(w).filter((t) => t.status !== "closing");
const tabName = (t: CloudTab) => `${t.agent_name} · ${t.runtime}`;

/** The tab last shown in each workspace, so going back lands there. */
const lastTabs = new Map<string, string>();
export function showCloudWorkspace(w: CloudWorkspace) {
  const last = lastTabs.get(w.id);
  cloudWorkspaces.open(w.id, last && tabsOf(w).some((t) => t.id === last) ? last : undefined);
}

// ---- the sidebar's group

/** The shown main view's way to open the create dialog, or, while none
 * is shown, that the next one should open with it. */
let creating: ((open: boolean) => void) | null = null;
let createAsked = false;
/** Open the create dialog (the sidebar's and the list's). */
export function newCloudWorkspace() {
  if (creating) creating(true);
  else {
    createAsked = true;
    cloudWorkspaces.open();
  }
}

export function CloudWorkspacesGroup({ enabled }: { enabled: boolean }) {
  const route = useCloudWorkspaces();
  const list = cloudWorkspaces.list;
  if (!enabled && !list.length) return null;
  return (
    <div class="ws-cloud-group" role="listbox" aria-label="Cloud workspaces" data-cloud-workspaces-group>
      <h3 class="ws-group cloud">
        <button class="ws-group-open" title="Every Cloud workspace" onClick={() => cloudWorkspaces.open()}>
          Cloud workspaces <span class="ws-group-count">{list.length}</span>
        </button>
      </h3>
      {list.map((w) => {
        const { word, dot } = cloudWorkspaceState(w);
        const tabs = tabsOf(w);
        const needs = tabs.some((t) => cloudTabAttention(t) === "needs_input");
        const current = route?.workspace === w.id;
        return (
          <div key={w.id} class={`ws-session${current ? " current" : ""}`} role="option" aria-selected={current} data-cloud-workspace={w.id} data-cloud-state={word}>
            <button class="ws-session-main" title={`${w.name}: ${word}, ${tabs.length} tab${tabs.length === 1 ? "" : "s"}`} onClick={() => showCloudWorkspace(w)}>
              <span class="ws-session-line">
                <span class={`ws-dot ${needs ? "needs_input" : dot}`} aria-hidden="true" />
                <span class="ws-session-name">{w.name}</span>
                {needs && <span class="attention" title="A tab needs you">●</span>}
              </span>
              <span class="ws-session-sub">
                <span class="ws-cloud-state">{word}</span>
                <span class="ws-session-ago">{tabs.length} tab{tabs.length === 1 ? "" : "s"}</span>
              </span>
            </button>
          </div>
        );
      })}
      {enabled && (
        <button class="ws-cloud-new" data-new-cloud-workspace onClick={newCloudWorkspace}>
          + Cloud workspace
        </button>
      )}
    </div>
  );
}

// ---- the session overview

/** The overview's cards for Cloud workspaces, after the machine's
 * sessions: each with its status and tabs. Opening one leaves the
 * overview (its route is replaced). */
export function CloudWorkspaceCards() {
  useCloudWorkspaces();
  const list = cloudWorkspaces.list;
  if (!list.length) return null;
  return (
    <>
      <h2 class="ws-ov-cloud-head">Cloud workspaces</h2>
      <div class="ws-ov-grid">
        {list.map((w) => {
          const { word, dot } = cloudWorkspaceState(w);
          return (
            <article key={w.id} class="ws-ov-session" data-overview-cloud-workspace={w.id}>
              <header class="ws-ov-session-head">
                <button class="ws-ov-name" title={`Show ${w.name}`} onClick={() => showCloudWorkspace(w)}>
                  <span class={`ws-dot ${dot}`} aria-hidden="true" />
                  <span class="ws-ellipsis">{w.name}</span>
                </button>
                <span class="ws-ov-counts">{word}</span>
              </header>
              <ul class="ws-ov-panes">
                {tabsOf(w).map((t) => (
                  <li key={t.id}>
                    <button class="ws-ov-pane" data-overview-cloud-tab={t.id} data-attention={cloudTabAttention(t)} title={`Go to this tab${cloudTabAttention(t) === "needs_input" ? " (this doesn't answer it)" : ""}\n${t.cwd}`} onClick={() => cloudWorkspaces.open(w.id, t.id)}>
                      <span class={`ws-dot ${cloudTabAttention(t)}`} aria-hidden="true" />
                      <span class="ws-ov-kind">{t.runtime}</span>
                      <span class="ws-ov-pane-name ws-ellipsis">{t.agent_name}</span>
                      <span class="ws-ov-where ws-ellipsis">{t.cwd}</span>
                    </button>
                  </li>
                ))}
              </ul>
            </article>
          );
        })}
      </div>
    </>
  );
}

// ---- the header

/** The header's crumbs for a Cloud workspace: the list, the workspace and
 * its status, and the tab. */
export function CloudWorkspaceCrumbs() {
  const route = useCloudWorkspaces();
  const w = cloudWorkspaces.find(route?.workspace ?? null);
  const tab = w && route?.tab ? tabsOf(w).find((t) => t.id === route.tab) : undefined;
  return (
    <>
      <button class="ws-crumb" onClick={() => cloudWorkspaces.open()} title="Every Cloud workspace">
        Cloud workspaces
      </button>
      {w && (
        <>
          <span class="ws-crumb-sep" aria-hidden="true">/</span>
          <button class="ws-crumb" onClick={() => cloudWorkspaces.open(w.id)} title={`${w.name}: its tabs and how it's set up`}>
            <span class="ws-crumb-name">{w.name}</span>
          </button>
          <span class={`ws-conn cloud ${cloudWorkspaceState(w).dot}`} role="status" data-cloud-status={cloudWorkspaceState(w).word}>
            <span class="ws-dot" aria-hidden="true" />
            {cloudWorkspaceState(w).word}
          </span>
        </>
      )}
      {tab && (
        <span class="ws-crumb-tab" title="This tab">
          <span class="ws-crumb-sep" aria-hidden="true">/</span>
          <span class="ws-crumb-tab-name">{tabName(tab)}</span>
        </span>
      )}
    </>
  );
}

/** Delete, in the header: the confirm names the tabs it ends. */
export function DeleteCloudWorkspace() {
  const route = useCloudWorkspaces();
  const w = cloudWorkspaces.find(route?.workspace ?? null);
  const [asking, setAsking] = useState(false);
  const [error, setError] = useState("");
  if (!w || w.status === "deleting") return null;
  const live = liveTabs(w);
  const go = async () => {
    try {
      await api(`workspaces/${w.id}`, "DELETE");
      setAsking(false);
      cloudWorkspaces.open();
      cloudWorkspaces.refresh();
    } catch (e) {
      setError((e as Error).message);
    }
  };
  return (
    <>
      <button class="ws-share" data-delete-cloud-workspace title="Delete this Cloud workspace, its machine and its tabs" onClick={() => { setError(""); setAsking(true); }}>
        Delete
      </button>
      {asking && (
        <ConfirmRemove title={`Delete ${w.name}?`} label="Delete" go={() => void go()} cancel={() => setAsking(false)}>
          <p>Its machine and everything on its disk are deleted: commit and push what you want to keep first.</p>
          {live.length ? (
            <>
              <p>This ends {live.length === 1 ? "its tab" : `its ${live.length} tabs`}:</p>
              <ul data-delete-ends>{live.map((t) => <li key={t.id}>{tabName(t)}</li>)}</ul>
            </>
          ) : (
            <p>It has no open tabs.</p>
          )}
          {error && <p role="alert" class="control-error">{error}</p>}
        </ConfirmRemove>
      )}
    </>
  );
}

// ---- the tab strip

export function CloudWorkspaceTabStrip() {
  const route = useCloudWorkspaces();
  const w = cloudWorkspaces.find(route?.workspace ?? null);
  const [adding, setAdding] = useState(false);
  if (!w) return null;
  const close = async (t: CloudTab) => {
    try {
      await api(`workspaces/${w.id}/tabs/${t.id}`, "DELETE");
      if (route?.tab === t.id) cloudWorkspaces.open(w.id);
      cloudWorkspaces.refresh();
    } catch { /* The next poll still shows it. */ }
  };
  return (
    <div class="tabstrip cloud-tabstrip">
      <div class="tabbar" role="tablist" aria-label={`Tabs in ${w.name}`}>
        {tabsOf(w).map((t) => {
          const selected = route?.tab === t.id;
          const attention = cloudTabAttention(t);
          return (
            <div key={t.id} class={selected ? "tab selected" : "tab"} role="tab" aria-selected={selected} data-cloud-tab={t.id} data-attention={attention} title={`${t.agent_name} on ${t.runtime} (${t.model}) in ${t.cwd}`} tabIndex={selected ? 0 : -1}
              onClick={() => { lastTabs.set(w.id, t.id); cloudWorkspaces.open(w.id, t.id); }}
              onKeyDown={(e) => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); cloudWorkspaces.open(w.id, t.id); } }}>
              <span class="host-tag" title={t.model}>{t.runtime}</span>
              <span class="tab-label">{t.agent_name}</span>
              {attention === "needs_input" ? <span class="attention" title="Needs you">●</span> : attention === "working" ? <span class="attention working" title="Working">●</span> : null}
              <button class="tab-close" title="Close tab: ends its conversation" aria-label={`Close tab ${t.agent_name}`} onClick={(e) => { e.stopPropagation(); void close(t); }}>×</button>
            </div>
          );
        })}
      </div>
      <button class="new-tab cloud-new-agent" data-new-cloud-tab disabled={w.status === "deleting"} title="Open an agent in this Cloud workspace" onClick={() => setAdding(true)}>
        + Agent
      </button>
      {adding && <NewCloudTabDialog workspace={w} close={() => setAdding(false)} />}
    </div>
  );
}

// ---- the main view

export function CloudWorkspaceMain() {
  const route = useCloudWorkspaces();
  const [creatingOpen, setCreating] = useState(() => {
    const asked = createAsked;
    createAsked = false;
    return asked;
  });
  useEffect(() => {
    creating = setCreating;
    return () => void (creating = null);
  }, []);
  const dialog = creatingOpen && <NewCloudWorkspaceDialog close={() => setCreating(false)} />;
  if (!route?.workspace) return <div class="cloud-ws-view" data-cloud-ws-list><CloudWorkspaceList create={() => setCreating(true)} />{dialog}</div>;
  const w = cloudWorkspaces.find(route.workspace);
  if (!w) {
    return (
      <div class="cloud-ws-view empty">
        <p>{cloudWorkspaces.loaded ? "This Cloud workspace isn't here any more." : "Loading…"}</p>
        {cloudWorkspaces.loaded && <button onClick={() => cloudWorkspaces.open()}>Every Cloud workspace</button>}
        {dialog}
      </div>
    );
  }
  const tab = route.tab ? tabsOf(w).find((t) => t.id === route.tab) : undefined;
  if (tab) lastTabs.set(w.id, tab.id);
  return (
    <div class="cloud-ws-view" data-cloud-ws={w.id}>
      <CloudWorkspaceFailure w={w} />
      {route.tab ? <CloudTabView key={route.tab} w={w} tab={tab} id={route.tab} /> : <CloudWorkspaceHome w={w} />}
      {dialog}
    </div>
  );
}

/** A failed setup: Managoat's failing step and the end of what it printed
 * (since v0.39.0 a failed script's `error` ends with its output's tail, so
 * it's shown as lines), and the way to its whole setup log. */
function CloudWorkspaceFailure({ w }: { w: CloudWorkspace }) {
  if (w.status !== "failed") return null;
  const showLog = () => {
    if (cloudWorkspaceRoute()?.tab) cloudWorkspaces.open(w.id);
    scrollToLog = true;
    requestAnimationFrame(() => {
      const log = document.querySelector<HTMLDetailsElement>("[data-cloud-setup-log]");
      if (log) { log.open = true; log.scrollIntoView({ block: "start" }); }
    });
  };
  return (
    <div class="cloud-ws-failed" role="alert" data-cloud-failed>
      <p><b>Setup failed.</b> Change its settings below if they need it, then Retry. A prompt to one of its tabs sets it up again too.</p>
      {w.error && <pre class="cloud-ws-error" data-cloud-error>{w.error}</pre>}
      <p><button class="link" data-show-setup-log onClick={showLog}>Show setup log</button></p>
      <RetryCloudWorkspace w={w} />
    </div>
  );
}

/** What the last setup printed, step by step: read from control when it's
 * opened (Managoat keeps it, so it reads without waking the machine), never
 * polled. Reopening reads it again. */
/** Show setup log asked for it: once it's read, bring it into view. */
let scrollToLog = false;

function CloudSetupLog({ w }: { w: CloudWorkspace }) {
  const [log, setLog] = useState<CloudSetupLog | null | undefined>(undefined);
  const [error, setError] = useState("");
  const shown = useRef<HTMLDetailsElement>(null);
  useEffect(() => {
    if (log === undefined || !scrollToLog) return;
    scrollToLog = false;
    shown.current?.scrollIntoView({ block: "start" });
  }, [log]);
  const read = () => {
    setError("");
    void api<{ log: CloudSetupLog | null }>(`workspaces/${w.id}/setup-log`).then((v) => setLog(v.log), (e: Error) => setError(e.message));
  };
  return (
    <details class="cloud-setup-log" data-cloud-setup-log ref={shown} onToggle={(e) => { if (e.currentTarget.open) read(); }}>
      <summary><h3>Setup log</h3></summary>
      {error ? <p role="alert" class="control-error">{error}</p>
        : log === undefined ? <p class="dim">Loading…</p>
        : !log ? <p class="dim">No setup has been kept yet.</p>
        : <>
          <p class="dim">{log.status ? `${log.status[0].toUpperCase()}${log.status.slice(1)}` : ""}{log.step ? ` at ${setupStepName(log.step)}` : ""}{log.finished_at ? ` · ${new Date(log.finished_at).toLocaleString()}` : ""} <button class="link" onClick={read}>Read again</button></p>
          {log.steps.map((step, i) => (
            <details key={`${step.step}-${i}`} class="cloud-setup-step" data-setup-step={step.step} data-setup-status={step.status} open={step.status === "failed"}>
              <summary><span class="mono">{setupStepName(step.step)}</span> <span class={`cloud-setup-status ${step.status ?? ""}`}>{step.status}</span></summary>
              {step.omitted_lines ? <p class="dim">{step.omitted_lines} earlier line{step.omitted_lines === 1 ? "" : "s"} left out.</p> : null}
              {step.output ? <pre class="cloud-ws-error">{step.output}</pre> : <p class="dim">It printed nothing.</p>}
            </details>
          ))}
        </>}
    </details>
  );
}

function CloudWorkspaceList({ create }: { create: () => void }) {
  const list = cloudWorkspaces.list;
  return (
    <>
      <div class="cloud-ws-head">
        <h2>Cloud workspaces</h2>
        <button class="primary" data-create-cloud-workspace onClick={create}>New Cloud workspace</button>
      </div>
      <p class="dim">A Cloud workspace is a machine in the cloud with your repositories on it. Each tab is an agent working there; they share its disk.</p>
      {cloudWorkspaces.error && <p role="alert" class="control-error">{cloudWorkspaces.error}</p>}
      {cloudWorkspaces.loaded && !list.length && !cloudWorkspaces.error && <p class="dim">You have no Cloud workspaces yet.</p>}
      <ul class="cloud-ws-list">
        {list.map((w) => (
          <li key={w.id}>
            <button class="cloud-ws-list-name" title={w.name} onClick={() => showCloudWorkspace(w)}>{w.name}</button>
            <span class="cloud-ws-list-state">{cloudWorkspaceState(w).word} · {tabsOf(w).length} tab{tabsOf(w).length === 1 ? "" : "s"}</span>
          </li>
        ))}
      </ul>
      <PastTabs />
    </>
  );
}

/** Closed tabs, read only: what a closed tab, or a deleted Cloud
 * workspace's, leaves to read. Read when the list is shown. */
function PastTabs() {
  const [tabs, setTabs] = useState<PastCloudTab[]>([]);
  useEffect(() => { void api<{ past_tabs: PastCloudTab[] }>("past-tabs").then((v) => setTabs(v.past_tabs), () => {}); }, []);
  if (!tabs.length) return null;
  return (
    <>
      <h3>Past tabs</h3>
      <p class="dim">Closed tabs, and the tabs of deleted Cloud workspaces: their transcripts, to read.</p>
      <ul class="cloud-ws-list" data-past-tabs>
        {tabs.map((t) => (
          <li key={t.id}>
            <button class="cloud-ws-list-name" data-past-tab={t.id} title={`${t.agent_name} in ${t.cloud_workspace_name}`} onClick={() => cloudPanes.open(t.id)}>{t.agent_name} · {t.runtime}</button>
            <span class="cloud-ws-list-state">{t.cloud_workspace_name}{t.cloud_workspace_deleted ? " (deleted)" : ""} · {new Date(t.closed).toLocaleDateString()}</span>
          </li>
        ))}
      </ul>
    </>
  );
}

function CloudWorkspaceHome({ w }: { w: CloudWorkspace }) {
  const spec = w.saved_spec;
  const tabs = tabsOf(w);
  const { word } = cloudWorkspaceState(w);
  return (
    <div class="cloud-ws-home">
      <h2>{w.name}</h2>
      <p class="dim" data-cloud-home-status>
        {word}
        {w.status === "provisioning" && ": installing packages, cloning repositories and running the setup script. Tabs opened now start once it's ready."}
        {w.status === "ready" && w.sandbox_status === "suspended" && ": its machine sleeps until a tab gets a prompt."}
      </p>
      <h3>Repositories</h3>
      {spec.repositories.length ? (
        <ul class="cloud-ws-repos">{spec.repositories.map((r) => <li key={r.mount}><span class="mono">{r.mount}</span> · {r.url}{r.ref ? ` @ ${r.ref}` : ""}</li>)}</ul>
      ) : (
        <p class="dim">None: tabs start in /home/sprite.</p>
      )}
      {(spec.packages.apt?.length || spec.packages.npm?.length) ? <p>Packages: {[...(spec.packages.apt ?? []).map((p) => `apt ${p}`), ...(spec.packages.npm ?? []).map((p) => `npm ${p}`)].join(", ")}</p> : null}
      <CloudWorkspaceSettings w={w} />
      <h3>Tabs</h3>
      {tabs.length ? (
        <ul class="cloud-ws-tabs">{tabs.map((t) => <li key={t.id}><button onClick={() => cloudWorkspaces.open(w.id, t.id)}>{tabName(t)}</button> <span class="dim">{t.status.replaceAll("_", " ")} · {t.cwd}</span></li>)}</ul>
      ) : (
        <p class="dim">No tabs yet. + Agent opens one.</p>
      )}
      <h3>Changes</h3>
      <CloudWorkspaceChanges w={w} />
      <CloudSetupLog w={w} />
    </div>
  );
}

const TAB_STATUS: Record<string, string> = { opening: "Opening", pending: "Starting", running: "Working", idle: "Idle", needs_input: "Needs you", failed: "Failed", terminated: "Ended", closing: "Closing" };

function CloudTabView({ w, tab, id }: { w: CloudWorkspace; tab?: CloudTab; id: string }) {
  const path = `workspaces/${w.id}/tabs/${id}`;
  const { item, events, error, setError, pending, answered, answer } = useCloudConversation<CloudTab>(path, () => false);
  const t = item ?? tab;
  const [prompt, setPrompt] = useState("");
  const [busy, setBusy] = useState(false);
  const [changes, setChanges] = useState(false);
  if (!t) return <div class="agent cloud-agent">{error ? <p role="alert">{error}</p> : <p class="dim">Loading…</p>}</div>;
  const ended = ["closing", "terminated"].includes(t.status);
  const send = async (e: Event) => {
    e.preventDefault();
    if (!prompt.trim()) return;
    setBusy(true); setError("");
    try { await api(`${path}/prompt`, "POST", { prompt }); setPrompt(""); cloudWorkspaces.refresh(); }
    catch (e) { setError((e as Error).message); } finally { setBusy(false); }
  };
  return (
    <div class="agent cloud-agent" data-cloud-tab-view={t.id}>
      <div class="agent-bar">
        <span class="agent-status">{TAB_STATUS[t.status] ?? t.status}</span>
        <span class="agent-name">{t.agent_name}</span>
        <span class="host-tag" title={t.model}>{t.runtime}</span>
        <span class="agent-spacer" />
        <button data-cloud-tab-changes aria-pressed={changes} title="What's changed on this Cloud workspace's disk" onClick={() => setChanges(!changes)}>Changes</button>
        {(t.status === "running" || t.status === "needs_input") && <button onClick={() => void api(`${path}/interrupt`, "POST", {}).catch((e: Error) => setError(e.message))}>Interrupt</button>}
      </div>
      <div class="agent-note">{w.name} · <span class="mono">{t.cwd}</span> · {t.model}</div>
      {t.waiting_for_setup && <p class="agent-note" data-waiting-for-setup>Waiting for the Cloud workspace to finish setting up; the first prompt starts then.</p>}
      {t.error && <p role="alert" class="control-error">{t.error}</p>}
      {error && <p role="alert">{error}</p>}
      {changes && <div class="cloud-tab-changes"><CloudWorkspaceChanges w={w} first={t.cwd} /></div>}
      <CloudTranscript events={events} pending={pending} answered={answered} ended={ended} answer={answer} setError={setError} />
      {!ended && t.status !== "opening" && (
        <form class="cloud-form cloud-tab-prompt" onSubmit={send}>
          <label>Prompt<textarea rows={3} value={prompt} onInput={(e) => setPrompt(e.currentTarget.value)} onKeyDown={(e) => { if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) void send(e); }} /></label>
          <button class="primary" disabled={busy || !prompt.trim() || t.status === "running"}>{busy ? "Sending…" : "Send"}</button>
        </form>
      )}
    </div>
  );
}

// ---- the dialogs

/** Words split on commas and spaces: "ffmpeg, jq" → ["ffmpeg", "jq"]. */
const words = (s: string) => s.split(/[\s,]+/).filter(Boolean);

type RepositoryDraft = { url: string; ref: string; secret: string };

function NewCloudWorkspaceDialog({ close }: { close: () => void }) {
  const [name, setName] = useState("");
  const [repositories, setRepositories] = useState<RepositoryDraft[]>([{ url: "", ref: "", secret: "" }]);
  const [apt, setApt] = useState("");
  const [npm, setNpm] = useState("");
  const [script, setScript] = useState("");
  const [secrets, setSecrets] = useState<string[]>([]);
  const [names, setNames] = useState<string[]>([]);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const requestKey = useRef(crypto.randomUUID());
  useEffect(() => { requestKey.current = crypto.randomUUID(); }, [name, repositories, apt, npm, script, secrets]);
  useEffect(() => { void api<{ names: string[] }>("secrets").then((v) => setNames(v.names), () => {}); }, []);
  const setRepository = (i: number, change: Partial<RepositoryDraft>) => setRepositories(repositories.map((r, j) => (j === i ? { ...r, ...change } : r)));
  const submit = async (e: Event) => {
    e.preventDefault(); setBusy(true); setError("");
    const spec: CloudWorkspaceSpec = {
      name: name.trim(),
      repositories: repositories.filter((r) => r.url.trim()).map((r) => ({ url: r.url.trim(), ...(r.ref.trim() ? { ref: r.ref.trim() } : {}), ...(r.secret ? { secret: r.secret } : {}) })),
      packages: { ...(words(apt).length ? { apt: words(apt) } : {}), ...(words(npm).length ? { npm: words(npm) } : {}) },
      ...(script.trim() ? { setup_script: script } : {}),
      ...(secrets.length ? { secrets } : {}),
    };
    try {
      const w = await api<CloudWorkspace>("workspaces", "POST", { request_key: requestKey.current, spec });
      requestKey.current = crypto.randomUUID();
      close();
      cloudWorkspaces.refresh();
      cloudWorkspaces.open(w.id);
    } catch (e) { setError((e as Error).message); } finally { setBusy(false); }
  };
  return (
    <div class="prompt-backdrop" role="dialog" aria-label="New Cloud workspace" onKeyDown={(e) => e.key === "Escape" && close()}>
      <div class="prompt cloud-launch">
        <h2>New Cloud workspace</h2>
        <p class="dim">A machine in the cloud with these repositories on it, set up once. Tabs you open there share its disk.</p>
        <form class="cloud-form" onSubmit={submit}>
          <label>Name<input required maxLength={120} value={name} onInput={(e) => setName(e.currentTarget.value)} /></label>
          <fieldset class="cloud-ws-repositories">
            <legend>Repositories</legend>
            {repositories.map((r, i) => (
              <div key={i} class="cloud-ws-repository" data-repository={i}>
                <label>Repository URL<input type="url" placeholder="https://github.com/owner/repository" value={r.url} onInput={(e) => setRepository(i, { url: e.currentTarget.value })} /></label>
                <label>Branch or tag (optional)<input value={r.ref} onInput={(e) => setRepository(i, { ref: e.currentTarget.value })} /></label>
                <label>Repository credential<select value={r.secret} onChange={(e) => setRepository(i, { secret: e.currentTarget.value })}><option value="">Public repository</option>{names.map((n) => <option key={n}>{n}</option>)}</select></label>
                {repositories.length > 1 && <button type="button" onClick={() => setRepositories(repositories.filter((_, j) => j !== i))}>Remove repository</button>}
              </div>
            ))}
            {repositories.length < 10 && <button type="button" onClick={() => setRepositories([...repositories, { url: "", ref: "", secret: "" }])}>Add a repository</button>}
          </fieldset>
          <label>apt packages (optional)<input placeholder="ffmpeg jq" value={apt} onInput={(e) => setApt(e.currentTarget.value)} /></label>
          <label>npm packages (optional)<input placeholder="pnpm typescript" value={npm} onInput={(e) => setNpm(e.currentTarget.value)} /></label>
          <label>Setup script (optional)<textarea rows={4} placeholder="cd /home/sprite/app && pnpm install" value={script} onInput={(e) => setScript(e.currentTarget.value)} /></label>
          {names.length > 0 && (
            <fieldset>
              <legend>Secrets the agents may use</legend>
              {names.map((n) => <label key={n}><input type="checkbox" checked={secrets.includes(n)} onChange={(e) => setSecrets(e.currentTarget.checked ? [...secrets, n] : secrets.filter((v) => v !== n))} />{n}</label>)}
            </fieldset>
          )}
          {error && <p role="alert" class="control-error">{error}</p>}
          <div class="prompt-buttons">
            <button type="button" onClick={close}>Cancel</button>
            <button class="primary" disabled={busy}>{busy ? "Creating…" : "Create Cloud workspace"}</button>
          </div>
        </form>
      </div>
    </div>
  );
}

function NewCloudTabDialog({ workspace: w, close }: { workspace: CloudWorkspace; close: () => void }) {
  const [agents, setAgents] = useState<CloudAgent[]>([]);
  const [models, setModels] = useState<CloudModel[]>([]);
  const [agent, setAgent] = useState("curated_coding");
  const [model, setModel] = useState(0);
  const mounts = cloudWorkspaceMounts(w);
  const [cwd, setCwd] = useState(mounts[0]);
  const [prompt, setPrompt] = useState("");
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const requestKey = useRef(crypto.randomUUID());
  useEffect(() => { requestKey.current = crypto.randomUUID(); }, [agent, model, cwd, prompt]);
  useEffect(() => {
    void api<{ agents: CloudAgent[] }>("agents").then((v) => { setAgents(v.agents); if (!v.agents.some((a) => a.id === agent) && v.agents[0]) setAgent(v.agents[0].id); }, (e: Error) => setError(e.message));
    void api<{ models: CloudModel[] }>("models").then((v) => setModels(v.models), (e: Error) => setError(e.message));
  }, []);
  const submit = async (e: Event) => {
    e.preventDefault(); setBusy(true); setError("");
    const pick = models[model];
    try {
      const t = await api<CloudTab>(`workspaces/${w.id}/tabs`, "POST", { request_key: requestKey.current, agent_id: agent, runtime: pick.runtime, model: pick.model, cwd, ...(prompt.trim() ? { prompt } : {}) });
      requestKey.current = crypto.randomUUID();
      close();
      cloudWorkspaces.refresh();
      cloudWorkspaces.open(w.id, t.id);
    } catch (e) { setError((e as Error).message); } finally { setBusy(false); }
  };
  return (
    <div class="prompt-backdrop" role="dialog" aria-label="Open an agent" onKeyDown={(e) => e.key === "Escape" && close()}>
      <div class="prompt cloud-launch">
        <h2>Open an agent in {w.name}</h2>
        <form class="cloud-form" onSubmit={submit}>
          <label>Agent<select value={agent} onChange={(e) => setAgent(e.currentTarget.value)}>{agents.map((a) => <option key={a.id} value={a.id}>{a.definition.name}{a.curated ? " · Arugula" : ""}</option>)}</select></label>
          <label>Runtime and model<select value={model} onChange={(e) => setModel(Number(e.currentTarget.value))}>{models.map((m, i) => <option key={`${m.runtime}:${m.model}`} value={i}>{m.runtime} · {m.model}</option>)}</select></label>
          <label>Starts in<select value={cwd} onChange={(e) => setCwd(e.currentTarget.value)}>{mounts.map((m) => <option key={m}>{m}</option>)}</select></label>
          <label>First prompt (optional)<textarea rows={4} value={prompt} onInput={(e) => setPrompt(e.currentTarget.value)} /></label>
          {w.status === "provisioning" && <p class="dim">The workspace is still setting up: the first prompt waits until it's ready.</p>}
          {error && <p role="alert" class="control-error">{error}</p>}
          <div class="prompt-buttons">
            <button type="button" onClick={close}>Cancel</button>
            <button class="primary" disabled={busy || !agents.length || !models.length}>{busy ? "Opening…" : "Open agent"}</button>
          </div>
        </form>
      </div>
    </div>
  );
}
