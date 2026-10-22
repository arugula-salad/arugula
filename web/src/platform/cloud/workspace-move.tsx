// Moving a beta Cloud workspace off Managoat (M189, #1250). Beta Cloud
// workspaces run on Managoat, which goes away: each one's page offers
// "Move to the new Cloud workspaces", which makes it again as a hosted
// workspace in the same project from its settings, with its secrets and its
// tabs' transcripts. Its disk doesn't move, so the dialog first lists each
// repo's uncommitted and unpushed work (read from its machine, never woken)
// and offers it as a download. The notice says the day the rest are
// deleted, when control has one (`ARUGULA_MANAGOAT_DELETE_AFTER`).

import { useEffect, useState } from "preact/hooks";
import type { CloudWorkspace } from "../../shared/cloud";
import { platformFetch, refusalOf } from "../../shared/maintenance";
import { useCloudApi } from "../../shared/platform-api";
import { names } from "../links";
import { pathOf } from "../router";
import { deletionDay, leftBehind, leftWords, type Leftover } from "./move";

type Moved = { id: string; project: string; name: string; done: boolean };
type Plan = {
  id: string; name: string; project: string; delete_after: string | null; can_move: boolean; why_not: string | null;
  moved: Moved | null; notes: string[]; transcripts: number; unread: string | null; repos: Leftover[];
  makes: { name: string; repos: string[]; packages: string[]; setup: boolean; env: string[]; secrets: string[] };
};
type Overview = { delete_after: string | null; can_move: boolean; why_not: string | null };
type MovedTo = { workspace: { id: string; name: string }; project: string; transcripts: number; secrets: string[]; notes: string[] };

let overview: Promise<Overview | null> | null = null;

/** The account's `GET /api/cloud/moving`, read once per page. */
function useOverview(): Overview | null {
  const api = useCloudApi();
  const [o, setO] = useState<Overview | null>(null);
  useEffect(() => {
    overview ??= api<Overview>("moving").catch(() => null);
    let live = true;
    void overview.then((v) => { if (live) setO(v); });
    return () => { live = false; };
  }, []);
  return o;
}

/** The notice on beta Cloud workspaces: the day they're deleted, when
 * control says one. */
export function CloudMoveNotice() {
  const o = useOverview();
  if (!o?.delete_after) return null;
  return (
    <div class="cloud-move-notice" role="status" data-cloud-move-notice={o.delete_after}>
      <p>
        <b>Beta Cloud workspaces are deleted on {deletionDay(o.delete_after)}</b>, with their machines and everything on their disks.
        Move each one to the new Cloud workspaces before then: its repos, setup, secrets and transcripts come with it.
        Work that isn't pushed doesn't, so download it first.
      </p>
    </div>
  );
}

const workspaceHref = (project: string, id: string) => pathOf({ page: "workspace", project, workspace: id }, names);

/** On a beta Cloud workspace's home: where it moved, or the move. */
export function MoveCloudWorkspace({ w }: { w: CloudWorkspace }) {
  const api = useCloudApi();
  const [plan, setPlan] = useState<Plan | null>(null);
  const [open, setOpen] = useState(false);
  const read = () => void api<Plan>(`workspaces/${w.id}/move?read=false`).then(setPlan, () => {});
  useEffect(read, [w.id]);
  if (!plan || w.status === "deleting") return null;
  if (plan.moved?.done) {
    return (
      <div class="cloud-move" data-cloud-moved={plan.moved.id}>
        <p>
          Moved to <a href={workspaceHref(plan.moved.project, plan.moved.id)}>{plan.moved.name}</a>, a new Cloud workspace.
          This one stays until beta workspaces are deleted{plan.delete_after ? ` on ${deletionDay(plan.delete_after)}` : ""}.
        </p>
      </div>
    );
  }
  return (
    <div class="cloud-move" data-cloud-move>
      <p>
        <button class="primary" data-move-cloud-workspace disabled={!plan.can_move} onClick={() => setOpen(true)}>Move to the new Cloud workspaces</button>
        {plan.why_not && <span class="dim"> {plan.why_not}</span>}
      </p>
      {open && <MoveDialog w={w} close={() => { setOpen(false); read(); }} />}
    </div>
  );
}

function MoveDialog({ w, close }: { w: CloudWorkspace; close: () => void }) {
  const api = useCloudApi();
  const [plan, setPlan] = useState<Plan | null>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [downloading, setDownloading] = useState(false);
  const [done, setDone] = useState<MovedTo | null>(null);
  useEffect(() => { void api<Plan>(`workspaces/${w.id}/move`).then(setPlan, (e: Error) => setError(e.message)); }, [w.id]);
  const download = async () => {
    setDownloading(true); setError("");
    try {
      const r = await platformFetch(`/api/cloud/workspaces/${w.id}/move/bundle`, { credentials: "same-origin" });
      if (!r.ok) throw new Error(refusalOf(r.status, await r.json().catch(() => ({}))));
      const name = /filename="([^"]+)"/.exec(r.headers.get("content-disposition") ?? "")?.[1] ?? "left-behind.tar";
      const url = URL.createObjectURL(await r.blob());
      const a = document.createElement("a");
      a.href = url; a.download = name; a.click();
      setTimeout(() => URL.revokeObjectURL(url), 1000);
    } catch (e) { setError((e as Error).message); } finally { setDownloading(false); }
  };
  const move = async () => {
    setBusy(true); setError("");
    try { setDone(await api<MovedTo>(`workspaces/${w.id}/move`, "POST", {})); }
    catch (e) { setError((e as Error).message); } finally { setBusy(false); }
  };
  const left = plan ? leftBehind(plan.repos) : [];
  return (
    <div class="prompt-backdrop" onClick={(e) => !busy && e.target === e.currentTarget && close()}>
      <div class="prompt control-prompt cloud-move-dialog" role="dialog" aria-modal="true" aria-label={`Move ${w.name}`} data-cloud-move-dialog>
        <h2>Move {w.name} to the new Cloud workspaces</h2>
        {done ? (
          <>
            <p data-cloud-move-done>
              <a href={workspaceHref(done.project, done.workspace.id)}>{done.workspace.name}</a> is being made: its box sets up from these settings in a pane you can watch.
              {done.transcripts > 0 && ` ${done.transcripts === 1 ? "Its tab's transcript is" : `Its ${done.transcripts} tabs' transcripts are`} there to read.`}
            </p>
            {done.notes.length > 0 && <ul class="cloud-move-notes">{done.notes.map((n) => <li key={n}>{n}</li>)}</ul>}
            <div class="prompt-buttons"><button class="primary" onClick={close}>Done</button></div>
          </>
        ) : !plan ? (
          error ? <p role="alert" class="control-error">{error}</p> : <p class="dim">Reading its machine…</p>
        ) : (
          <>
            <h3>Left behind</h3>
            <p>The disk doesn't move. What isn't pushed stays here, and is deleted with it{plan.delete_after ? ` on ${deletionDay(plan.delete_after)}` : ""}.</p>
            {plan.unread ? <p class="dim" data-cloud-move-unread>{plan.unread}</p>
              : !plan.repos.length ? <p class="dim">It has no repositories.</p>
              : (
                <ul class="cloud-move-repos" data-cloud-move-left={left.length}>
                  {plan.repos.map((r) => (
                    <li key={r.mount} data-cloud-move-repo={r.mount}>
                      <span class="mono">{r.mount}</span>{r.branch && <span class="dim"> · {r.branch}</span>}
                      {r.error ? <p class="control-error">{r.error}</p> : <p class="dim">{leftWords(r)}</p>}
                    </li>
                  ))}
                </ul>
              )}
            {!plan.unread && left.length > 0 && (
              <p><button data-cloud-move-download disabled={downloading} onClick={() => void download()}>{downloading ? "Reading…" : "Download what's left behind (.tar)"}</button></p>
            )}
            <h3>What comes with it</h3>
            <ul class="cloud-move-makes">
              <li>A new Cloud workspace, <b>{plan.makes.name}</b>, in the same project.</li>
              {plan.makes.repos.length > 0 && <li>{plan.makes.repos.length === 1 ? "Its repo" : `Its ${plan.makes.repos.length} repos`}, cloned fresh.</li>}
              {(plan.makes.packages.length > 0 || plan.makes.setup) && <li>Its packages and setup script, run once in a pane on its first boot.</li>}
              {plan.makes.secrets.length > 0 && <li>Its secrets: <span class="mono">{plan.makes.secrets.join(", ")}</span>.</li>}
              {plan.transcripts > 0 && <li>{plan.transcripts === 1 ? "Its tab's transcript" : `Its ${plan.transcripts} tabs' transcripts`}, to read.</li>}
            </ul>
            {plan.notes.length > 0 && (
              <>
                <h3>What changes</h3>
                <ul class="cloud-move-notes">{plan.notes.map((n) => <li key={n}>{n}</li>)}</ul>
              </>
            )}
            <p class="dim">Agents start fresh there. This one stays as it is until it's deleted.</p>
            {plan.why_not && <p class="control-error">{plan.why_not}</p>}
            {error && <p role="alert" class="control-error">{error}</p>}
            <div class="prompt-buttons">
              <button data-cancel-move onClick={close}>Cancel</button>
              <button class="primary" data-confirm-move disabled={busy || !plan.can_move} onClick={() => void move()}>{busy ? "Moving…" : "Move"}</button>
            </div>
          </>
        )}
      </div>
    </div>
  );
}
