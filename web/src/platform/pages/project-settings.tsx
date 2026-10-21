// `/p/:project/settings`: a project's settings (M163). For now its linked
// GitHub repos: who links one is a member who maintains it on GitHub, and
// the platform checks that. Where the GitHub App isn't installed, linking
// sends the person to GitHub to install it, and GitHub sends them back to
// `/installed`, which comes here with the repo filled in.

import { useEffect, useState } from "preact/hooks";
import type { LinkedRepo, LinkedRepos } from "../../shared/proto.gen";
import { platformFetch } from "../../shared/maintenance";
import { useSubscribe } from "../../shared/ui/hooks";
import { names } from "../links";
import { go, pathOf, REPO } from "../router";
import { session } from "../session";
import { Missing } from "./project";

/** A call to the platform: its JSON, or the platform's `{error}` thrown
 * with the rest of the answer. */
async function call<T>(method: string, path: string, body?: unknown): Promise<T> {
  const r = await platformFetch(path, {
    method,
    credentials: "same-origin",
    headers: body === undefined ? {} : { "Content-Type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const v = (await r.json().catch(() => ({}))) as { error?: string; reason?: string; install_url?: string };
  if (!r.ok) throw Object.assign(new Error(v.error ?? `Request failed (${r.status})`), { answer: v });
  return v as T;
}

const hours = (ms: number) => Math.max(1, Math.ceil((ms - Date.now()) / 3_600_000));

export function ProjectSettings({ id, link }: { id: string; link?: string }) {
  useSubscribe((fn) => session.subscribe(fn));
  const p = session.project(id);
  const [repos, setRepos] = useState<LinkedRepo[] | null>(null);
  const [repo, setRepo] = useState(link ?? "");
  const [err, setErr] = useState("");
  const [busy, setBusy] = useState(false);
  const base = `/api/projects/${encodeURIComponent(id)}/repos`;
  const load = () =>
    call<LinkedRepos>("GET", base).then(
      (v) => setRepos(v.repos),
      (e: Error) => setErr(e.message),
    );
  useEffect(() => void load(), [id]);
  if (!p) return <Missing what="That project isn't one of yours (or it's gone)." />;
  const changes = !p.team || p.team.role !== "viewer";
  const act = async (f: () => Promise<unknown>) => {
    setBusy(true);
    setErr("");
    try {
      await f();
      await load();
    } catch (e) {
      const a = (e as { answer?: { reason?: string; install_url?: string } }).answer;
      // The App isn't installed where the repo lives: off to GitHub, which
      // brings them back to `/installed`.
      if (a?.reason === "install_app" && a.install_url) location.href = a.install_url;
      else setErr((e as Error).message);
    } finally {
      setBusy(false);
    }
  };
  const submit = (e: Event) => {
    e.preventDefault();
    const r = repo.trim();
    if (!REPO.test(r)) return setErr("A repo on GitHub: OWNER/NAME");
    void act(async () => {
      await call("POST", base, { repo: r });
      setRepo("");
    });
  };
  return (
    <section class="platform-section" data-project-settings={p.id}>
      <h1>{p.name}: settings</h1>
      <h2>Linked repos</h2>
      <p class="dim">
        A linked GitHub repo's pull requests and issues live here, for the project's members. Linking one takes admin or maintain on it on GitHub. Unlinking stops reading
        it at once, and what was read of it goes within a day.
      </p>
      {repos?.length === 0 && <p class="dim">None yet.</p>}
      <ul class="platform-list" data-linked-repos>
        {(repos ?? []).map((r) => (
          <li key={r.repo_id} data-linked-repo={r.full_name}>
            {r.full_name}
            {r.private && <span class="dim"> · private</span>}
            {r.purge_after !== null ? (
              <>
                <span class="dim"> · unlinked, gone within {hours(r.purge_after)}h</span>{" "}
                {changes && (
                  <button class="control-linkish" disabled={busy} onClick={() => act(() => call("POST", base, { repo: r.full_name }))}>
                    Link again
                  </button>
                )}
              </>
            ) : (
              changes && (
                <>
                  {" "}
                  <button class="control-linkish" data-unlink={r.full_name} disabled={busy} onClick={() => act(() => call("DELETE", `${base}/${r.full_name}`))}>
                    Unlink
                  </button>
                </>
              )
            )}
          </li>
        ))}
      </ul>
      {changes ? (
        <form class="platform-link-repo" aria-label="Link a repo" onSubmit={submit}>
          <input required aria-label="Repo" placeholder="OWNER/NAME" value={repo} data-link-repo onInput={(e) => setRepo(e.currentTarget.value)} />
          <button class="primary" disabled={busy}>
            Link
          </button>
        </form>
      ) : (
        <p class="dim">A team's owners and editors link repos.</p>
      )}
      {err && <p class="control-error" data-link-error>{err}</p>}
    </section>
  );
}

/** Back from installing the GitHub App: on to the project's settings, with
 * the repo filled in, to link it. */
export function Installed({ project, repo }: { project?: string; repo?: string }) {
  useEffect(() => {
    go(project ? pathOf({ page: "project-settings", project, link: repo }, names) : "/", true);
  }, [project, repo]);
  return <p class="platform-section dim">Back from GitHub…</p>;
}
