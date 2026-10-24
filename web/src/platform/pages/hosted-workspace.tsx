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
import { follow, pathOf } from "../router";
import { controlOrigin, session } from "../session";
import { toneClass, workspaceTone, type Tone } from "../swarm";
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

/** A project's hosted workspaces: null until they're read, and none where
 * the platform has none. */
export function useHostedWorkspaces(id: string): HostedWorkspace[] | null {
  const [list, setList] = useState<HostedWorkspace[] | null>(null);
  useEffect(() => {
    let live = true;
    get<{ workspaces: HostedWorkspace[] }>(`${project(id)}/workspaces`).then(
      (v) => live && setList(v.workspaces),
      () => live && setList([]),
    );
    return () => void (live = false);
  }, [id]);
  return list;
}

/** The links to a project's hosted workspaces, each a card in the swarm's
 * language (../swarm.css): its bar is its state, its mono line its id and
 * its machine's. */
export function HostedWorkspaceList({ project: id, list }: { project: string; list: HostedWorkspace[] }) {
  return (
    <ul class="platform-list sw-cards sw-grid" data-hosted-workspaces-list>
      {list.map((w) => {
        const to = pathOf({ page: "workspace", project: id, workspace: w.id }, names);
        return (
          <li key={w.id}>
            <a class={`sw-card ${toneClass(workspaceTone(w.state, w.asked))}`} href={to} data-hosted-workspace-link={w.id} onClick={(e) => follow(e, to)}>
              <span class="sw-head">
                <b class="sw-name">{w.name}</b>
              </span>
              <span class="sw-id">
                {w.id} · {w.machine ?? "no machine yet"}
              </span>
              <span class="dim">
                {said(w)}
                {w.rebuild_needed ? " · wants a rebuild" : ""}
              </span>
            </a>
          </li>
        );
      })}
    </ul>
  );
}

/** How many of them are in each state, as the swarm's bar counts its
 * panes: the ready, the ones being made or woken, the paused, the failed. */
function WorkspaceStats({ list }: { list: HostedWorkspace[] }) {
  const n = (t: Tone) => list.filter((w) => workspaceTone(w.state, w.asked) === t).length;
  const stats: [number, string][] = [
    [n("ok"), "ready"],
    [n("warn"), "on the way"],
    [list.filter((w) => w.state === "paused" && w.asked !== "wake").length, "paused"],
    [n("danger"), "failed"],
  ];
  return (
    <div class="sw-stats" data-hosted-stats>
      <span>
        <b>{list.length}</b> {list.length === 1 ? "workspace" : "workspaces"}
      </span>
      {stats.map(([k, what]) =>
        k ? (
          <span key={what} class={what === "failed" ? "bad" : undefined}>
            <b>{k}</b> {what}
          </span>
        ) : null,
      )}
    </div>
  );
}

/** `/p/:project/workspaces`: the project's hosted workspaces. */
export function HostedWorkspaces({ project: id }: { project: string }) {
  const list = useHostedWorkspaces(id);
  return (
    <section class="platform-section sw-page hosted-list" data-hosted-workspaces>
      <h1>Workspaces</h1>
      {list?.length ? <WorkspaceStats list={list} /> : null}
      {list === null ? <p class="dim">Loading…</p> : list.length ? <HostedWorkspaceList project={id} list={list} /> : <NoWorkspaces project={id} />}
    </section>
  );
}

/** What a project with no hosted workspaces says. */
export function NoWorkspaces({ project: id }: { project: string }) {
  return (
    <p class="dim" data-no-workspaces>
      None yet. <code>arugula cloud create NAME --project {id}</code> makes one, as do agents with Arugula's <code>cloud</code> tool.
    </p>
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
      <div class="hosted-ws-view">
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
                  <span class="dim sw-mono">
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
