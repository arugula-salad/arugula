// A Cloud workspace's Changes (S40, M118, #1075): per repository, what
// `git status` and `git diff` say on its machine, and a file opened from
// there. Read through control's `/api/cloud/workspaces/{id}/…` reads, which
// never wake a parked machine; nor does this view, which reads only while
// the machine is ready, when it's shown, when a tab's turn ends, and on
// Refresh. Nothing polls: a read can keep a provider's machine awake.

import { useEffect, useState } from "preact/hooks";
import { cloudWorkspaces, useCloudApi, type CloudTab, type CloudWorkspace } from "../../shared/cloud";

type GitChange = { path: string; index: string; worktree: string; renamed_from?: string | null };
type GitStatus = { path: string; repo_root: string; branch: string | null; entries: GitChange[]; truncated: boolean };
type GitDiff = { repo_root: string; diff: string; truncated: boolean };
type CloudFile = { path: string; size: number; truncated: boolean; encoding: "utf-8" | "base64"; content: string };
type RepositoryChanges = { mount: string; status?: GitStatus; diff?: GitDiff; error?: string };

type DiffLine = { kind: "add" | "del" | "ctx" | "at" | "eol"; text: string };
type FileDiff = { path: string; old?: string; binary: boolean; add: number; del: number; lines: DiffLine[] };

/** A unified diff, one entry per file. */
function parseUnifiedDiff(text: string): FileDiff[] {
  const files: FileDiff[] = [];
  let file: FileDiff | undefined, hunk = false;
  for (const line of text.split("\n")) {
    const head = /^diff --git a\/(.*) b\/(.*)$/.exec(line);
    if (head) {
      file = { path: head[2], old: head[1] !== head[2] ? head[1] : undefined, binary: false, add: 0, del: 0, lines: [] };
      files.push(file); hunk = false;
      continue;
    }
    if (!file) continue;
    if (line.startsWith("@@")) { hunk = true; file.lines.push({ kind: "at", text: line }); }
    else if (!hunk) { if (line.startsWith("Binary files ")) file.binary = true; }
    else if (line.startsWith("+")) { file.add++; file.lines.push({ kind: "add", text: line.slice(1) }); }
    else if (line.startsWith("-")) { file.del++; file.lines.push({ kind: "del", text: line.slice(1) }); }
    else if (line.startsWith("\\")) file.lines.push({ kind: "eol", text: line.slice(2) });
    else if (line.startsWith(" ")) file.lines.push({ kind: "ctx", text: line.slice(1) });
  }
  return files;
}

/** git's state of one side, as the one letter git status prints. */
const LETTER: Record<string, string> = { modified: "M", type_changed: "T", added: "A", deleted: "D", renamed: "R", copied: "C", unmerged: "U", untracked: "?", ignored: "!" };
const CLASS: Record<string, string> = { added: "added", untracked: "untracked", deleted: "deleted", renamed: "renamed", copied: "renamed" };
/** The side that changed: the disk's, else the index's. */
const changed = (c: GitChange) => (c.worktree !== "unchanged" ? c.worktree : c.index);

const query = (params: Record<string, string>) => new URLSearchParams(params).toString();

/** Whether the machine can be read now without waking it. */
const cloudWorkspaceReadable = (w: CloudWorkspace) => w.status === "ready" && w.sandbox_status !== "suspended";

/** `tabs`: the workspace's tabs, where they aren't in the `cloudWorkspaces`
 * store (a Cloud workspace block's, #1117). */
export function CloudWorkspaceChanges({ w, first, tabs }: { w: CloudWorkspace; first?: string; tabs?: CloudTab[] }) {
  const api = useCloudApi();
  const mounts = w.saved_spec.repositories.map((r) => r.mount ?? "").filter(Boolean);
  // The repository holding `first` (a tab's cwd) first.
  const holds = (m: string) => !!first && (first === m || first.startsWith(`${m}/`));
  mounts.sort((a, b) => Number(holds(b)) - Number(holds(a)));
  const readable = cloudWorkspaceReadable(w);
  // Each tab's status: a turn that ends is when files change.
  const turns = (tabs ?? cloudWorkspaces.tabs.get(w.id) ?? []).map((t) => `${t.id}:${t.status === "running" ? "running" : "rest"}`).join();
  const [repos, setRepos] = useState<RepositoryChanges[]>([]);
  const [loading, setLoading] = useState(false);
  const [asked, setAsked] = useState(0);
  const [file, setFile] = useState<{ path: string; file?: CloudFile; error?: string } | null>(null);
  const key = mounts.join("\n");
  useEffect(() => {
    if (!readable || !mounts.length) return;
    let live = true;
    setLoading(true);
    void Promise.all(mounts.map(async (mount): Promise<RepositoryChanges> => {
      try {
        const [status, diff] = await Promise.all([
          api<GitStatus>(`workspaces/${w.id}/git-status?${query({ path: mount, untracked: "all" })}`),
          api<GitDiff>(`workspaces/${w.id}/diff?${query({ path: mount, ref: "HEAD" })}`),
        ]);
        return { mount, status, diff };
      } catch (e) { return { mount, error: (e as Error).message }; }
    })).then((r) => { if (live) { setRepos(r); setLoading(false); } });
    return () => { live = false; };
  }, [w.id, readable, key, turns, asked]);
  const open = async (path: string) => {
    setFile({ path });
    try { setFile({ path, file: await api<CloudFile>(`workspaces/${w.id}/file?${query({ path })}`) }); }
    catch (e) { setFile({ path, error: (e as Error).message }); }
  };
  if (!mounts.length) return <p class="dim" data-cloud-changes>No repositories, so there are no changes to show.</p>;
  if (!readable) {
    return (
      <p class="dim" data-cloud-changes data-cloud-changes-unread>
        {w.status === "ready" ? "Parked: its changes show once a prompt to one of its tabs wakes the machine. Looking here doesn't wake it."
          : w.status === "provisioning" ? "Changes show once it's set up." : "There's no machine to read changes from."}
      </p>
    );
  }
  return (
    <div class="cloud-changes" data-cloud-changes>
      <div class="cloud-changes-bar">
        <span class="dim">{loading ? "Reading…" : "Uncommitted changes, against each repository's HEAD"}</span>
        <button data-cloud-changes-refresh disabled={loading} onClick={() => setAsked((n) => n + 1)}>Refresh</button>
      </div>
      {repos.map((r) => <Repository key={r.mount} r={r} open={(p) => void open(p)} />)}
      {file && (
        <div class="cloud-changes-file" data-cloud-file={file.path}>
          <div class="cloud-changes-bar">
            <b class="mono cloud-changes-path">{file.path}</b>
            {file.file && <span class="dim">{file.file.size} bytes</span>}
            <button onClick={() => setFile(null)}>Close file</button>
          </div>
          {file.error ? <p role="alert" class="control-error">{file.error}</p>
            : !file.file ? <p class="dim">Reading…</p>
            : file.file.encoding === "base64" ? <p class="dim">A binary file.</p>
            : <pre class="cloud-changes-content">{file.file.content}</pre>}
          {file.file?.truncated && <p class="diff-note">Only the start of this file is shown.</p>}
        </div>
      )}
    </div>
  );
}

function Repository({ r, open }: { r: RepositoryChanges; open: (path: string) => void }) {
  const diffs = r.diff ? parseUnifiedDiff(r.diff.diff) : [];
  const entries = r.status?.entries ?? [];
  const root = r.status?.repo_root ?? r.mount;
  return (
    <section class="cloud-changes-repo" data-cloud-changes-repo={r.mount}>
      <h4>
        <span class="mono">{r.mount}</span>
        {r.status?.branch && <span class="dim"> · {r.status.branch}</span>}
        {r.status && <span class="dim"> · {entries.length ? `${entries.length} changed` : "no changes"}</span>}
      </h4>
      {r.error && <p role="alert" class="control-error">{r.error}</p>}
      {entries.length > 0 && (
        <ul class="cloud-changes-status">
          {entries.map((c) => (
            <li key={c.path}>
              <span class={`diff-st st-${CLASS[changed(c)] ?? "modified"}`} title={`index ${c.index}, worktree ${c.worktree}`}>{LETTER[changed(c)] ?? "M"}</span>
              <button class="cloud-changes-open" title={`Open ${c.path}`} disabled={changed(c) === "deleted"} onClick={() => open(`${root}/${c.path}`)}>
                {c.renamed_from && <span class="dim">{c.renamed_from} → </span>}{c.path}
              </button>
            </li>
          ))}
        </ul>
      )}
      {r.status?.truncated && <p class="diff-note">More changed than is listed here.</p>}
      {diffs.map((f) => (
        <details key={f.path} class="diff-file cloud-changes-diff" open={f.add + f.del <= 400} data-cloud-diff-file={f.path}>
          <summary class="diff-file-head">
            <span class="diff-path">{f.old && <span class="dim">{f.old} → </span>}{f.path}</span>
            {f.binary ? <span class="dim">binary</span> : <><span class="adds">+{f.add}</span><span class="dels">−{f.del}</span></>}
          </summary>
          <div class="diff-hunks">
            {f.lines.map((l, i) => l.kind === "at" ? <div key={i} class="diff-at">{l.text}</div> : (
              <div key={i} class={`dl ${l.kind}`}>
                <span class="mk">{l.kind === "add" ? "+" : l.kind === "del" ? "-" : ""}</span>
                <span class="code">{l.text}</span>
              </div>
            ))}
          </div>
        </details>
      ))}
      {r.diff?.truncated && <p class="diff-note">The diff is longer than is shown here.</p>}
    </section>
  );
}
