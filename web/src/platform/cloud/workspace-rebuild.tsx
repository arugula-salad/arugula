// Rebuild and Retry for a Cloud workspace (S40, M120, #1077). Rebuild sets
// a new machine up from the saved settings and wipes the disk, so it asks
// first and says so (decision 9); the tabs keep their transcripts. Managoat
// refuses it while a tab is mid-turn, unless the person picks Interrupt and
// rebuild. Retry sets a failed setup up again: there's no machine, so
// nothing to warn about. Progress shows through the status poll.

import { useEffect, useState } from "preact/hooks";
import { cloudWorkspaceRoute, cloudWorkspaces, useCloudPanes, type CloudTab, type CloudWorkspace } from "../../shared/cloud";
import { platformFetch, refusalOf } from "../../shared/maintenance";

/** A tab status Managoat counts as a turn running (or about to). */
const MID_TURN = ["pending", "running", "needs_input"];
const tabName = (t: CloudTab) => `${t.agent_name} · ${t.runtime}`;

/** POST (or DELETE) to control, keeping the refusal's reason. A
 * maintenance window (M141) is waited out. */
async function post(path: string, body: unknown, method = "POST"): Promise<{ error?: string; reason?: string }> {
  try {
    const r = await platformFetch(`/api/cloud/${path}`, { method, credentials: "same-origin", headers: { "Content-Type": "application/json" }, body: body === undefined ? undefined : JSON.stringify(body) });
    if (r.ok) return {};
    const value = await r.json().catch(() => ({}));
    return { error: refusalOf(r.status, value), reason: value.reason };
  } catch (e) {
    return { error: (e as Error).message };
  }
}

/** Rebuild, in the header (`place="header"`) and in Settings. Shown while
 * the Cloud workspace is ready; a failed one has Retry instead. */
export function RebuildCloudWorkspace({ w: given, place }: { w?: CloudWorkspace; place: "header" | "settings" }) {
  useCloudPanes();
  const w = given ?? cloudWorkspaces.find(cloudWorkspaceRoute()?.workspace ?? null);
  const [asking, setAsking] = useState(false);
  if (!w || w.status !== "ready") return null;
  if (w.rebuild_queued) {
    return (
      <span class="cloud-ws-rebuild-waiting" role="status" data-rebuild-waiting={place} title="Managoat has no machine free; control keeps asking for ten minutes">
        Rebuild waiting for a machine <CancelQueuedRebuild w={w} class={place === "header" ? "ws-share" : undefined} />
      </span>
    );
  }
  return (
    <>
      <button class={place === "header" ? "ws-share" : w.stale ? "primary" : undefined} data-rebuild-cloud-workspace={place}
        title="Set a new machine up from the saved settings. The disk is wiped." onClick={() => setAsking(true)}>
        Rebuild
      </button>
      {asking && <RebuildConfirm w={w} close={() => setAsking(false)} />}
    </>
  );
}

/** Cancel a rebuild or retry waiting for a machine. Too late once control
 * has sent it: control says so (`rebuild_started`). */
function CancelQueuedRebuild({ w, class: cls }: { w: CloudWorkspace; class?: string }) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const go = async () => {
    setBusy(true); setError("");
    const r = await post(`workspaces/${w.id}/rebuild`, undefined, "DELETE");
    setBusy(false);
    if (r.error && r.reason !== "not_waiting") setError(r.error);
    cloudWorkspaces.refresh();
  };
  return (
    <>
      <button class={cls} data-cancel-queued-rebuild disabled={busy} onClick={() => void go()}>{busy ? "Cancelling…" : "Cancel"}</button>
      {error && <span role="alert" class="control-error" data-cancel-queued-rebuild-error>{error}</span>}
    </>
  );
}

/** How long the dialog ignores clicks after it opens, as `ConfirmRemove`. */
const SETTLE_MS = 500;

function RebuildConfirm({ w, close }: { w: CloudWorkspace; close: () => void }) {
  const [ready, setReady] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  /** Control's refusal naming the tabs mid-turn, once it has said so. */
  const [refused, setRefused] = useState("");
  useEffect(() => {
    const t = setTimeout(() => setReady(true), SETTLE_MS);
    return () => clearTimeout(t);
  }, []);
  const running = (cloudWorkspaces.tabs.get(w.id) ?? []).filter((t) => MID_TURN.includes(t.status));
  const interrupting = running.length > 0 || !!refused;
  const n = w.pending?.length ?? 0;
  const go = async (interrupt: boolean) => {
    setBusy(true); setError("");
    const r = await post(`workspaces/${w.id}/rebuild`, interrupt ? { interrupt: true } : {});
    setBusy(false);
    if (!r.error) { close(); cloudWorkspaces.refresh(); return; }
    if (r.reason === "turns_running") setRefused(r.error);
    else setError(r.error);
  };
  return (
    <div class="prompt-backdrop" onClick={(e) => ready && !busy && e.target === e.currentTarget && close()}>
      <div class="prompt control-prompt" role="alertdialog" aria-modal="true" aria-label={`Rebuild ${w.name}?`} data-rebuild-confirm>
        <h2>Rebuild {w.name}?</h2>
        <p data-rebuild-warning>
          <b>The disk is wiped.</b> A new machine is set up from the saved settings, and nothing on the old one is kept: uncommitted and untracked work, anything outside the repositories, and each agent's memory of its conversation. Commit and push what you want to keep first.
        </p>
        <p>The tabs stay open with their transcripts. Each agent starts fresh on its next prompt.{n > 0 && ` The machine gets the ${n === 1 ? "saved change" : `${n} saved changes`}.`}</p>
        {interrupting && (
          <div class="cloud-ws-mid-turn" role="status" data-rebuild-mid-turn>
            <p>{refused || `${running.map(tabName).join(", ")} ${running.length === 1 ? "is" : "are"} mid-turn.`}</p>
            <p>Managoat won't rebuild while a turn runs. Wait for it to finish, or interrupt it: its transcript so far is kept.</p>
            {!refused && running.length > 0 && <ul>{running.map((t) => <li key={t.id}>{tabName(t)}</li>)}</ul>}
          </div>
        )}
        {error && <p role="alert" class="control-error">{error}</p>}
        <div class="prompt-buttons">
          <button data-cancel-rebuild onClick={close}>Cancel</button>
          {interrupting ? (
            <button class="danger" data-confirm-rebuild="interrupt" disabled={!ready || busy} onClick={() => void go(true)}>{busy ? "Rebuilding…" : "Interrupt and rebuild"}</button>
          ) : (
            <button class="danger" data-confirm-rebuild disabled={!ready || busy} onClick={() => void go(false)}>{busy ? "Rebuilding…" : "Rebuild"}</button>
          )}
        </div>
      </div>
    </div>
  );
}

/** Retry, on a failed Cloud workspace: its setup again, from the settings
 * as saved. There's no machine, so nothing to lose and no confirm. */
export function RetryCloudWorkspace({ w }: { w: CloudWorkspace }) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  if (w.status !== "failed") return null;
  const go = async () => {
    setBusy(true); setError("");
    const r = await post(`workspaces/${w.id}/retry`, {});
    setBusy(false);
    if (r.error) setError(r.error);
    else cloudWorkspaces.refresh();
  };
  return (
    <div class="cloud-ws-retry">
      <button class="primary" data-retry-cloud-workspace disabled={busy || w.rebuild_queued} onClick={() => void go()}>
        {busy ? "Retrying…" : w.rebuild_queued ? "Retry waiting…" : "Retry"}
      </button>
      {error && <p role="alert" class="control-error">{error}</p>}
      <CloudWorkspaceRebuildNote w={w} />
    </div>
  );
}

/** A rebuild or retry waiting for a machine, or why the last one that
 * waited didn't happen (while one is still wanted). */
export function CloudWorkspaceRebuildNote({ w }: { w: CloudWorkspace }) {
  const what = w.status === "failed" ? "Retry" : "Rebuild";
  if (w.rebuild_queued) return <p class="dim" role="status" data-rebuild-queued>{what} is waiting for Managoat to have a machine free. Control keeps asking for ten minutes.{w.status === "failed" && <> <CancelQueuedRebuild w={w} /></>}</p>;
  if (w.rebuild_error && (w.stale || w.status === "failed")) return <p role="alert" class="control-error" data-rebuild-error>{w.rebuild_error}</p>;
  return null;
}
