// Cloud workspaces (M179, M184) on the platform page: `/p/:project/workspaces`
// lists a project's, and `/p/:project/workspaces/hw_…` shows one. Their
// one name on every page is "cloud workspace", as the CLI's `arugula cloud`
// and the MCP `cloud` tool say (UI audit #184); the code's is "hosted". A
// cloud workspace is a machine of the project ("A hosted machine is arugulad in a
// rented sandbox"): its terminals and agent blocks are live on control's
// page, where the machines are, and this page has none (it holds no keys).
// So + Agent and its panes open there (`#machine=<id>`, the shell's), and
// this page shows what the platform keeps: the workspace, its machine, and
// its closed agents' transcripts, read only (`/api/projects/…/transcripts`).
// Opening one asks the platform to wake a paused box. Create a cloud
// workspace makes one, a name and a size, as `arugula cloud create` does,
// for those the platform lets. Its Edit, Rebuild, previews, secrets and the
// project's inference keys are `./hosted-manage`'s, as `arugula cloud`'s
// edit, rebuild, preview, secret and key (Discovery 10).

import { useEffect, useState } from "preact/hooks";
import { platformFetch } from "../../shared/maintenance";
import { HOSTED_LABEL, HOSTED_MEANS } from "../../shared/hosted";
import { CopyText } from "../../shared/ui/copy";
import { Markdown } from "../../shared/ui/markdown";
import { usePlatformApi } from "../../shared/platform-api";
import { PageHeader } from "../../shared/ui/page-header";
import { StateChip } from "../../shared/ui/state-chip";
import { names } from "../links";
import { follow, go, pathOf } from "../router";
import { controlOrigin, session, type PlatformProject } from "../session";
import { Modal } from "../../shared/ui/modal";
import { useSubscribe } from "../../shared/ui/hooks";
import { toneClass, workspaceTone, type Tone } from "../swarm";
import { Missing } from "./project";
import { Time } from "../../shared/ui/time";
import { SIZES, sizeOf, type Size } from "../settings/workspace-size";
import { mayWrite } from "./issues";
import { InferenceKeys, Previews, Secrets, WorkspaceActions, type Spec } from "./hosted-manage";

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
  /** What its box is built from (M266's size, and what Edit changes). */
  spec: Spec;
  /** Its snapshot (M263): what its new boxes start from, set up. */
  snapshot?: Snapshot;
}

/** A workspace's snapshot: the latest build's status, when the current one
 * was made (null: there's none), and the latest build's error. */
interface Snapshot {
  status: "building" | "ready" | "failed";
  built_at: number | null;
  error?: string;
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

/** A machine's id, short: the card has no room for all of it. */
const shortMachine = (m: string) => (m.length > 8 ? `${m.slice(0, 8)}…` : m);

/** States nothing changes on its own: the page stops looking again. Not
 * paused: opening the page wakes it, and the first answer can come back
 * before the wake is asked, so it keeps looking until it's up. */
const SETTLED = new Set(["ready", "failed"]);

const size = (n: number) => (n < 1024 ? `${n} B` : n < 1024 * 1024 ? `${Math.round(n / 1024)} KiB` : `${(n / 1024 / 1024).toFixed(1)} MiB`);

/** What its new boxes start from (M263): a snapshot made from its spec,
 * set up, or the template, and why the latest build failed. */
function SnapshotLine({ s }: { s: Snapshot }) {
  const made = s.built_at ? (
    <>
      New boxes start from a snapshot made <Time at={s.built_at} on />, set up.
    </>
  ) : (
    "New boxes run their setup."
  );
  return (
    <div data-hosted-snapshot={s.status}>
      <p class="dim">
        {made}
        {s.status === "building" ? " Making a new snapshot…" : ""}
      </p>
      {s.status === "failed" && s.error && <p class="control-error">The latest snapshot wasn't made: {s.error}</p>}
    </div>
  );
}

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

/** The links to a project's cloud workspaces, each a card in the swarm's
 * language (../swarm.css): its bar is its state, its mono line its id and
 * its machine's, labelled (UI audit #162), and a chip when it wants a
 * rebuild (UI audit #165). */
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
                <span title={w.id}>{w.id.slice(0, 11)}</span> ·{" "}
                {w.machine ? <span title={`Its machine: ${w.machine}`}>machine {shortMachine(w.machine)}</span> : "no machine yet"}
              </span>
              <span class="dim">{said(w)}</span>
              {w.rebuild_needed && (
                <span class="sw-chip" title="Its spec changed since its box was made" data-hosted-rebuild-chip>
                  Wants a rebuild
                </span>
              )}
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
  useSubscribe((fn) => session.subscribe(fn));
  const list = useHostedWorkspaces(id);
  return (
    <section class="platform-section sw-page hosted-list" data-hosted-workspaces>
      <PageHeader
        title="Cloud workspaces"
        actions={!!list?.length && makesWorkspaces(session.project(id)) && <NewWorkspaceButton project={id} />}
        stats={list?.length ? <WorkspaceStats list={list} /> : null}
      />
      {list === null ? <p class="dim">Loading…</p> : list.length ? <HostedWorkspaceList project={id} list={list} /> : <NoWorkspaces project={id} />}
    </section>
  );
}

/** Whether the reader may make one of `p`'s cloud workspaces, as the
 * platform's create route allows: an owner or an editor (not a team's
 * viewer), on the cloud workspaces' list. */
export function makesWorkspaces(p: PlatformProject | undefined): boolean {
  return !!p && session.hosted && (!p.team || p.team.role !== "viewer");
}

/** What a project with no cloud workspaces says: what one is, and Create a
 * cloud workspace where the reader may make one, else why not; the
 * command that makes one from a terminal on a line of its own, to copy
 * (UI audit #162). */
export function NoWorkspaces({ project: id }: { project: string }) {
  const p = session.project(id);
  const makes = makesWorkspaces(p);
  return (
    <div class="hosted-none" data-no-workspaces>
      <p>A cloud workspace is a machine of the project's in the cloud: its terminals and agents keep going with your computer off, and everyone in the project reaches them.</p>
      {makes ? (
        <p>
          <NewWorkspaceButton project={id} />
        </p>
      ) : (
        <p class="dim" data-no-workspaces-why>
          {!session.hosted ? "Cloud workspaces are in a limited beta, and your account isn't in it yet." : "A team's owners and editors make its cloud workspaces."}
        </p>
      )}
      {makes && (
        <>
          <p class="dim">Or from a terminal, or ask an agent with Arugula's <code>cloud</code> tool to:</p>
          <CopyText text={`arugula cloud create NAME --project ${id}`} />
        </>
      )}
    </div>
  );
}

/** A cloud workspace's name, as the platform takes it (hosted::good_name). */
export const WORKSPACE_NAME = /^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$/;

/** Create a cloud workspace: a button, and its dialog. */
export function NewWorkspaceButton({ project: id, primary = true }: { project: string; primary?: boolean }) {
  const [open, setOpen] = useState(false);
  return (
    <>
      <button type="button" class={primary ? "primary" : "secondary"} data-hosted-new aria-haspopup="dialog" onClick={() => setOpen(true)}>
        Create a cloud workspace
      </button>
      {open && <NewWorkspace project={id} close={() => setOpen(false)} />}
    </>
  );
}

/** Its dialog: a name and a size, made as `arugula cloud create` makes one
 * (`POST /api/projects/{project}/workspaces`); then its page, which
 * follows it while its box is made. */
function NewWorkspace({ project: id, close }: { project: string; close: () => void }) {
  const [name, setName] = useState("");
  const [size, setSize] = useState<Size>("small");
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState("");
  const submit = async (e: Event) => {
    e.preventDefault();
    const n = name.trim();
    if (!WORKSPACE_NAME.test(n)) return setErr("A name: letters, digits, '-', '_' and '.', up to 64, starting with a letter or digit.");
    setBusy(true);
    setErr("");
    try {
      const r = await platformFetch(`${project(id)}/workspaces`, {
        method: "POST",
        credentials: "same-origin",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ name: n, spec: { size } }),
      });
      const v = (await r.json().catch(() => ({}))) as { id?: string; error?: string };
      if (!r.ok || !v.id) throw new Error(v.error ?? `Request failed (${r.status})`);
      close();
      go(pathOf({ page: "workspace", project: id, workspace: v.id }, names));
    } catch (e) {
      setErr((e as Error).message);
      setBusy(false);
    }
  };
  return (
    <Modal close={close} class="hosted-new">
      <form onSubmit={(e) => void submit(e)} data-hosted-new-form>
        <h2>Create a cloud workspace</h2>
        <p class="dim">Its box is made now and joins the project in a minute or two. Its repos, packages and setup script come after, with Edit on its page or <code>arugula cloud edit</code>.</p>
        <label>
          Name
          <input autofocus required value={name} placeholder="dev" data-hosted-new-name onInput={(e) => setName(e.currentTarget.value)} />
        </label>
        <fieldset class="hosted-new-sizes">
          <legend>Size</legend>
          {SIZES.map((s) => (
            <label key={s.size} class="control-check">
              <input type="radio" name="size" value={s.size} checked={size === s.size} data-hosted-new-size={s.size} onChange={() => setSize(s.size)} />
              <span>
                {s.label} <span class="dim">· {s.per_hour} an hour while it runs</span>
              </span>
            </label>
          ))}
        </fieldset>
        {err && (
          <p class="control-error" role="alert" data-hosted-new-error>
            {err}
          </p>
        )}
        <div class="prompt-buttons">
          <button type="button" class="secondary" onClick={close}>
            Cancel
          </button>
          <button type="submit" class="primary" disabled={busy} data-hosted-new-create>
            {busy ? "Creating…" : "Create"}
          </button>
        </div>
      </form>
    </Modal>
  );
}

/** One cloud workspace: its state, its ids, + Agent, and its closed
 * agents' transcripts. */
export function HostedWorkspacePage({ project: id, workspace }: { project: string; workspace: string }) {
  const api = usePlatformApi();
  const [w, setW] = useState<HostedWorkspace | null>(null);
  const [err, setErr] = useState("");
  const [transcripts, setTranscripts] = useState<Transcript[] | null>(null);
  const [open, setOpen] = useState<{ id: string; text: string } | null>(null);
  useEffect(() => {
    let live = true;
    let t: ReturnType<typeof setInterval> | undefined;
    const load = () => {
      get<HostedWorkspace>(`${project(id)}/workspaces/${encodeURIComponent(workspace)}`).then(
        (v) => {
          if (!live) return;
          setW(v);
          setErr("");
          // Settled, with nothing asked of it: it won't change on its own
          // (UI audit #165).
          if (!v.asked && SETTLED.has(v.state)) clearInterval(t);
        },
        (e: Error) => live && setErr(e.message),
      );
      get<{ transcripts: Transcript[] }>(`${project(id)}/transcripts?workspace=${encodeURIComponent(workspace)}`).then(
        (v) => live && setTranscripts(v.transcripts),
        () => live && setTranscripts([]),
      );
    };
    load();
    // While it's being made or woken, look again.
    t = setInterval(load, 5000);
    return () => {
      live = false;
      clearInterval(t);
    };
  }, [id, workspace]);
  const p = session.project(id);
  if (!p) return <Missing what="That project isn't one of yours (or it's gone)." />;
  if (err && !w) return <Missing what={err} />;
  if (!w) return <p class="platform-section dim">Loading…</p>;
  // Its panes are there to open only while it's up (UI audit #165).
  const ready = w.state === "ready" && !w.asked;
  const list = pathOf({ page: "workspaces", project: id }, names);
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
    <section class="platform-section sw-page hosted-workspace" data-hosted-workspace={w.id}>
      <PageHeader
        title={w.name}
        named
        back={{ href: list, label: "Cloud workspaces", onClick: (e) => follow(e, list) }}
        state={
          <StateChip tone={workspaceTone(w.state, w.asked)} data-hosted-state={w.state}>
            {said(w)}
          </StateChip>
        }
        actions={
          <>
            <WorkspaceActions p={p} w={w} changed={(v) => v && setW({ ...w, ...v })} />
            {w.machine && w.state === "ready" && (
              <button type="button" class="primary" onClick={() => window.open(machineHref(w.machine ?? "", true), "_blank", "noopener")} data-hosted-agent>
                + Agent ↗
              </button>
            )}
          </>
        }
        sub={
          <>
            {/* Its ids, which its card shows short (UI audit #165). */}
            <p class="hosted-ids sw-mono" data-hosted-ids>
              <code>{w.id}</code>
              <span data-hosted-size={sizeOf(w.spec ?? {})}>
                {` · ${sizeOf(w.spec ?? {})}, ${SIZES.find((s) => s.size === sizeOf(w.spec ?? {}))?.box}`}
              </span>
              {w.machine && (
                <>
                  {" · machine "}
                  <code>{w.machine}</code>
                </>
              )}
            </p>
            {/* A chip, and what it means as a sentence (UI audit #184). */}
            <p class="hosted-label">
              <span class="hosted-chip">{HOSTED_LABEL}</span> {HOSTED_MEANS[0].toUpperCase() + HOSTED_MEANS.slice(1)}
            </p>
          </>
        }
      />
      {w.error && <p class="control-error">{w.error}</p>}
      {w.rebuild_needed && (
        <div class="hosted-rebuild" data-hosted-rebuild>
          <p>
            <span class="sw-chip">Wants a rebuild</span> Its spec changed since its box was made.{" "}
            {mayWrite(p) && session.hosted ? "Rebuild it here, or from a terminal" : "An owner or editor rebuilds it, here or from a terminal"}: the
            command checks its repos for unpushed work first, and stops if there is some.
          </p>
          <CopyText text={`arugula cloud rebuild ${w.name} --project ${id}`} />
        </div>
      )}
      {w.snapshot && <SnapshotLine s={w.snapshot} />}
      <div class="hosted-ws-view">
        <section data-hosted-live>
          <h2>Panes and agents</h2>
          {w.machine ? (
            <p>
              Its terminals and agent blocks are live on Control, where your machines are, and everyone in the project sees the same ones.{" "}
              {ready ? (
                <a href={machineHref(w.machine)} target="_blank" rel="noopener" data-hosted-open>
                  Open them ↗
                </a>
              ) : (
                <span class="dim" data-hosted-open-later>
                  Once it's ready, they open from here.
                </span>
              )}
            </p>
          ) : (
            <p class="dim">Its box hasn't joined the project yet.</p>
          )}
          <p class="dim">
            Agents elsewhere reach it too: an agent on one of your machines passes <code>cloud_workspace: "{w.name}"</code> to Arugula's
            tools (run, start_agent, wait, read_output).
          </p>
        </section>
        <section data-hosted-transcripts>
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
                    {t.agent}
                    {t.closed_at ? (
                      <>
                        {" "}
                        · closed <Time at={t.closed_at} />
                      </>
                    ) : null}{" "}
                    · {size(t.bytes)}
                    {t.cut ? " · its middle was cut" : ""}
                  </span>
                  {/* Its Markdown drawn, as chat's is (UI audit #165). */}
                  {open?.id === t.id && (
                    <div class="hosted-transcript">
                      <Markdown text={open.text} refs={null} />
                    </div>
                  )}
                </li>
              ))}
            </ul>
          )}
        </section>
      </div>
      <div class="hosted-ws-view hosted-manage">
        <Previews p={p} w={w} />
        <Secrets p={p} w={w} />
        <InferenceKeys p={p} />
      </div>
    </section>
  );
}
