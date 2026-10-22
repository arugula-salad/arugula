// Hosted workspaces (M179, M184) on the platform page: `/p/:project/workspaces`
// lists a project's, and `/p/:project/workspaces/hw_…` shows one. A hosted
// workspace is a machine of the project ("A hosted machine is arugulad in a
// rented sandbox"): its terminals and agent blocks are live on control's
// page, where the machines are, and this page has none (it holds no keys).
// So + Agent and its panes open there (`#machine=<id>`, the shell's), and
// this page shows what the platform keeps: the workspace, its machine, and
// its closed agents' transcripts, read only (`/api/projects/…/transcripts`).
// Opening one asks the platform to wake a paused box.

import { useEffect, useState } from "preact/hooks";
import { platformFetch } from "../../shared/maintenance";
import { HOSTED_LABEL, HOSTED_MEANS } from "../../shared/hosted";
import { usePlatformApi } from "../../shared/platform-api";
import { names } from "../links";
import { pathOf } from "../router";
import { controlOrigin, session } from "../session";
import { Missing } from "./project";

/** A hosted workspace, as the platform answers it. */
interface HostedWorkspace {
  id: string;
  project: string;
  name: string;
  state: string;
  asked: string | null;
  machine: string | null;
  error: string | null;
  rebuild_needed: boolean;
  created_at: number;
}

/** A closed agent's transcript, without its text. */
interface Transcript {
  id: string;
  workspace: string;
  block: number;
  agent: string;
  title: string;
  cwd: string | null;
  started_at: number;
  closed_at: number;
  cut: boolean;
  bytes: number;
}

async function get<T>(path: string): Promise<T> {
  const r = await platformFetch(path, { credentials: "same-origin" });
  const v = (await r.json().catch(() => ({}))) as { error?: string };
  if (!r.ok) throw new Error(v.error ?? `Request failed (${r.status})`);
  return v as T;
}

const project = (id: string) => `/api/projects/${encodeURIComponent(id)}`;

/** Where control's page opens a machine, and with `agent`, its Start an
 * agent. */
export function machineHref(machine: string, agent = false): string {
  return `${controlOrigin()}/#machine=${encodeURIComponent(machine)}${agent ? "&agent" : ""}`;
}

const STATES: Record<string, string> = {
  creating: "Making its box…",
  enrolling: "Its box is joining the project…",
  ready: "Ready",
  paused: "Paused: opening it wakes it",
  waking: "Waking…",
  failed: "Failed",
  deleting: "Being deleted",
};

const said = (w: HostedWorkspace) => (w.asked === "wake" ? STATES.waking : (STATES[w.state] ?? w.state));

const when = (ms: number) => (ms ? new Date(ms).toLocaleString() : "");

const size = (n: number) => (n < 1024 ? `${n} B` : n < 1024 * 1024 ? `${Math.round(n / 1024)} KiB` : `${(n / 1024 / 1024).toFixed(1)} MiB`);

/** A project's hosted workspaces, above its Cloud workspaces: none, where
 * the platform has none. */
export function HostedWorkspaces({ project: id }: { project: string }) {
  const [list, setList] = useState<HostedWorkspace[]>([]);
  useEffect(() => {
    let live = true;
    get<{ workspaces: HostedWorkspace[] }>(`${project(id)}/workspaces`).then(
      (v) => live && setList(v.workspaces),
      () => live && setList([]),
    );
    return () => void (live = false);
  }, [id]);
  if (!list.length) return null;
  return (
    <section class="platform-section hosted-list" data-hosted-workspaces>
      <h2>Hosted workspaces</h2>
      <ul>
        {list.map((w) => (
          <li key={w.id}>
            <a href={pathOf({ page: "workspace", project: id, workspace: w.id }, names)} data-hosted-workspace-link={w.id}>
              {w.name}
            </a>{" "}
            <span class="dim">{said(w)}</span>
          </li>
        ))}
      </ul>
    </section>
  );
}

/** One hosted workspace: its state, its machine, + Agent, and its closed
 * agents' transcripts. */
export function HostedWorkspacePage({ project: id, workspace }: { project: string; workspace: string }) {
  const api = usePlatformApi();
  const [w, setW] = useState<HostedWorkspace | null>(null);
  const [err, setErr] = useState("");
  const [transcripts, setTranscripts] = useState<Transcript[] | null>(null);
  const [open, setOpen] = useState<{ id: string; text: string } | null>(null);
  useEffect(() => {
    let live = true;
    const load = () => {
      get<HostedWorkspace>(`${project(id)}/workspaces/${encodeURIComponent(workspace)}`).then(
        (v) => live && (setW(v), setErr("")),
        (e: Error) => live && setErr(e.message),
      );
      get<{ transcripts: Transcript[] }>(`${project(id)}/transcripts?workspace=${encodeURIComponent(workspace)}`).then(
        (v) => live && setTranscripts(v.transcripts),
        () => live && setTranscripts([]),
      );
    };
    load();
    // While it's being made or woken, look again.
    const t = setInterval(load, 5000);
    return () => {
      live = false;
      clearInterval(t);
    };
  }, [id, workspace]);
  const p = session.project(id);
  if (!p) return <Missing what="That project isn't one of yours (or it's gone)." />;
  if (err && !w) return <Missing what={err} />;
  if (!w) return <p class="platform-section dim">Loading…</p>;
  const up = w.machine ? api.machine(w.machine)?.up : undefined;
  const readTranscript = async (t: Transcript) => {
    if (open?.id === t.id) return setOpen(null);
    try {
      const v = await get<{ text: string }>(`${project(id)}/transcripts/${encodeURIComponent(t.id)}`);
      setOpen({ id: t.id, text: v.text });
    } catch (e) {
      api.toast((e as Error).message);
    }
  };
  return (
    <section class="platform-workspace hosted-workspace" data-hosted-workspace={w.id}>
      <nav class="platform-ws-head" aria-label="Where you are">
        <a class="tab" href={pathOf({ page: "workspaces", project: id }, names)}>
          Workspaces
        </a>
        <span class="dim">›</span>
        <b>{w.name}</b>
        <span class="dim" data-hosted-state={w.state}>
          {said(w)}
        </span>
        <span class="bar-fill" />
        {w.machine && w.state === "ready" && (
          <button type="button" class="primary" onClick={() => window.open(machineHref(w.machine ?? "", true), "_blank", "noopener")} data-hosted-agent>
            + Agent ↗
          </button>
        )}
      </nav>
      <div class="cloud-ws-view">
        <section class="platform-section">
          <p class="hosted-label" title={HOSTED_MEANS}>
            {HOSTED_LABEL}: {HOSTED_MEANS}
          </p>
          {w.error && <p class="control-error">{w.error}</p>}
          {w.rebuild_needed && <p class="dim">Its spec changed since its box was made: a rebuild makes it again.</p>}
        </section>
        <section class="platform-section" data-hosted-live>
          <h2>Panes and agents</h2>
          {w.machine ? (
            <p>
              Its terminals and agent blocks are live on control's page, where your machines are, and everyone in the project sees the same
              ones{up === undefined ? "" : up ? " (it's up)" : " (it's asleep)"}.{" "}
              <a href={machineHref(w.machine)} target="_blank" rel="noopener" data-hosted-open>
                Open them ↗
              </a>
            </p>
          ) : (
            <p class="dim">Its box hasn't joined the project yet.</p>
          )}
          <p class="dim">
            Agents elsewhere reach it too: an agent on one of your machines passes <code>cloud_workspace: "{w.name}"</code> to Arugula's
            tools (run, start_agent, wait, read_output).
          </p>
        </section>
        <section class="platform-section" data-hosted-transcripts>
          <h2>Closed agents</h2>
          {transcripts === null ? (
            <p class="dim">Loading…</p>
          ) : !transcripts.length ? (
            <p class="dim">When an agent block on it closes, its transcript is kept here, read only.</p>
          ) : (
            <ul class="hosted-transcripts">
              {transcripts.map((t) => (
                <li key={t.id} data-transcript={t.id}>
                  <button type="button" class="link" onClick={() => void readTranscript(t)} aria-expanded={open?.id === t.id}>
                    {t.title || t.agent}
                  </button>{" "}
                  <span class="dim">
                    {t.agent} · closed {when(t.closed_at)} · {size(t.bytes)}
                    {t.cut ? " · its middle was cut" : ""}
                  </span>
                  {open?.id === t.id && <pre class="hosted-transcript">{open.text}</pre>}
                </li>
              ))}
            </ul>
          )}
        </section>
      </div>
    </section>
  );
}
