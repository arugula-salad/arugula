// `/p/:project/settings`: a project's settings (M163). For now its linked
// GitHub repos: who links one is a member who maintains it on GitHub, and
// the platform checks that. Where the GitHub App isn't installed, linking
// sends the person to GitHub to install it, and GitHub sends them back to
// `/installed`, which comes here with the repo filled in. Each linked repo
// has a switch for content mirroring (M166), with what turning it on means.
// Then where its new issues go (M195, moved here by UI audit #163), its
// environments, their secrets and "Deploy…" (M252), its standing grant for
// runs (M211), its cloud workspaces' sizes (M266) and finish step for agent
// sessions (M261),
// for its owners the agent tokens over it
// and its own secrets (M230), what its Claude calls cost (M231), and the
// apps products keep in it (M236), and for a team's owners its chat's
// owners-only switches (UI audit #168). Each part is a section
// (../settings/section), as /settings's are: a heading, one line, the rest
// behind More, and on a wide screen an index of them beside the column.
// The repos are a table, a row each with its switch. After them, the Plane
// projects connected to it (#356, ../settings/plane), with boards on.

import { useEffect, useRef, useState } from "preact/hooks";
import type { LinkedRepo, LinkedRepos } from "../../shared/proto.gen";
import { platformFetch } from "../../shared/maintenance";
import { desktopApp } from "../../shared/desktop";
import { BrowserStep } from "../../shared/ui/browser-step";
import { ConfirmRemove } from "../../shared/ui/confirm";
import { useSubscribe } from "../../shared/ui/hooks";
import { PageHeader } from "../../shared/ui/page-header";
import { names } from "../links";
import { follow, go, pathOf, REPO } from "../router";
import { session } from "../session";
import { useBoards } from "./boards";
import { Missing, REPOS_MEAN } from "./project";
import { IssueHomeSetting } from "./issues";
import { RunGrant } from "../settings/runs";
import { FinishStep } from "../settings/finish-step";
import { AgentModel } from "../settings/agent-model";
import { WorkspaceSize } from "../settings/workspace-size";
import { Environments } from "../settings/environments";
import { ProjectAgents } from "../settings/project-agents";
import { ClaudeSpend } from "../settings/spend";
import { ClientApps } from "../settings/client-apps";
import { TeamChat } from "../settings/chat-alerts";
import { PlaneConnections } from "../settings/plane";
import { BoardSettings } from "../settings/board-settings";
import { Section, SectionIndex } from "../settings/section";

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

/** What Keep bodies does: its column's and each switch's tooltip. */
const KEEP =
  "Keeps its pull requests' and issues' bodies and comments here, so the project can search them. People sometimes paste secrets into comments. Takes admin or maintain on GitHub.";

export function ProjectSettings({ id, link }: { id: string; link?: string }) {
  useSubscribe((fn) => session.subscribe(fn));
  const p = session.project(id);
  // Plane is Labs with boards (#356): shown once the boards flag is known on.
  const boards = useBoards(id);
  const [repos, setRepos] = useState<LinkedRepo[] | null>(null);
  const [repo, setRepo] = useState(link ?? "");
  const [err, setErr] = useState("");
  const [busy, setBusy] = useState(false);
  // Unlink asks first (UI audit #158): what was read of the repo goes.
  const [unlinking, setUnlinking] = useState<string | null>(null);
  // The desktop app's way through the install: GitHub opens in the browser
  // and this asks again, with the request that was refused, when the window
  // is back (#236).
  const [install, setInstall] = useState<{ url: string; retry: () => Promise<unknown> } | null>(null);
  const body = useRef<HTMLDivElement>(null);
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
      // brings them back to `/installed`. The desktop app has no tab to
      // come back to: it opens GitHub in the browser and waits here (#236).
      if (a?.reason === "install_app" && a.install_url) {
        if (desktopApp()) setInstall({ url: a.install_url, retry: f });
        else location.href = a.install_url;
      } else setErr((e as Error).message);
    } finally {
      setBusy(false);
    }
  };
  const submit = (e: Event) => {
    e.preventDefault();
    const r = repo.trim();
    if (!REPO.test(r)) return setErr("A repo on GitHub: owner/name");
    void act(async () => {
      await call("POST", base, { repo: r });
      setRepo("");
    });
  };
  // The head counts the linked ones, not those waiting for the purge.
  const linked = (repos ?? []).filter((r) => r.purge_after === null).length;
  const home = pathOf({ page: "project", project: p.id }, names);
  return (
    <section class="platform-section sw-page settings-page" data-project-settings={p.id}>
      <div class="settings-layout">
        <SectionIndex body={body} />
        <div class="settings-body" ref={body}>
          <PageHeader title="Project settings" back={{ href: home, label: p.name, onClick: (e) => follow(e, home) }} />
          <Section
            id="repos"
            title="Linked repos"
            count={repos ? linked : undefined}
            lede="GitHub repos whose pull requests and issues live in this project, for its members."
            more={
              <>
                <p>
                  Linking a repo takes admin or maintain on it on GitHub, and so does keeping its bodies. Unlinking stops reading it at once, and
                  what was read of it goes within a day.
                </p>
                <p data-mirror-notice>
                  Otherwise a PR's or issue's body and comments are read from GitHub when someone opens it, and kept a few minutes in memory.
                  Keep bodies holds the bodies, comments and review comments of the repo's pull requests and issues here, so the project can
                  search their text, until you turn it off (they're deleted at once) or unlink the repo; they're deleted within 24 hours of the
                  GitHub App being uninstalled. People sometimes paste secrets into comments.
                </p>
              </>
            }
          >
            <div data-linked-repos>
              {repos?.length === 0 && (
                <p class="dim" data-linked-repos-none>
                  {REPOS_MEAN} {changes ? "Link one below: its name on GitHub, owner/name." : ""}
                </p>
              )}
              {repos?.length ? (
                <table class="settings-table repo-table">
                  <thead>
                    <tr>
                      <th scope="col">Repo</th>
                      <th scope="col" title={KEEP}>
                        Keep bodies
                      </th>
                      <th scope="col">
                        <span class="settings-hidden">Unlink</span>
                      </th>
                    </tr>
                  </thead>
                  <tbody>
                    {repos.map((r) => (
                      <tr key={r.repo_id} data-linked-repo={r.full_name}>
                        <th scope="row">
                          <span class="repo-name">{r.full_name}</span>
                          {r.private && <span class="settings-tag">private</span>}
                        </th>
                        {r.purge_after !== null ? (
                          <>
                            <td class="repo-keep">
                              <span class="dim">unlinked, gone within {hours(r.purge_after)}h</span>
                            </td>
                            <td class="settings-row-act">
                              {changes && (
                                <button class="secondary small" disabled={busy} onClick={() => act(() => call("POST", base, { repo: r.full_name }))}>
                                  Link again
                                </button>
                              )}
                            </td>
                          </>
                        ) : (
                          <>
                            <td class="repo-keep" data-col="Keep bodies">
                              <input
                                type="checkbox"
                                role="switch"
                                class="sw-switch"
                                aria-label={`Keep ${r.full_name}'s bodies and comments here`}
                                title={KEEP}
                                data-mirror-content={r.full_name}
                                checked={r.mirror_content}
                                disabled={busy || !changes}
                                onChange={(e) => {
                                  const on = e.currentTarget.checked;
                                  void act(() => call("PATCH", `${base}/${r.full_name}`, { mirror_content: on }));
                                }}
                              />
                            </td>
                            <td class="settings-row-act">
                              {changes && (
                                <button class="secondary danger small" data-unlink={r.full_name} disabled={busy} onClick={() => setUnlinking(r.full_name)}>
                                  Unlink
                                </button>
                              )}
                            </td>
                          </>
                        )}
                      </tr>
                    ))}
                  </tbody>
                </table>
              ) : null}
            </div>
            {changes ? (
              <form class="platform-link-repo" aria-label="Link a repo" onSubmit={submit}>
                <input
                  required
                  aria-label="Repo"
                  placeholder="Owner/name"
                  value={repo}
                  data-link-repo
                  // The project home's Link a repo comes here, to type one.
                  autofocus={location.hash === "#repos"}
                  onInput={(e) => setRepo(e.currentTarget.value)}
                />
                <button class="primary" disabled={busy}>
                  Link
                </button>
              </form>
            ) : (
              <p class="dim">A team's owners and editors link repos.</p>
            )}
            {install && (
              <BrowserStep
                url={install.url}
                label="Install the GitHub App"
                cancel={() => setInstall(null)}
                check={async () => {
                  // Linked once the App is installed; until then the same 409.
                  try {
                    await install.retry();
                    return true;
                  } catch (e) {
                    if ((e as { answer?: { reason?: string } }).answer?.reason === "install_app") return false;
                    throw e;
                  }
                }}
                onDone={() => {
                  setInstall(null);
                  void load();
                }}
              />
            )}
            {err && <p class="control-error" data-link-error>{err}</p>}
            {unlinking && (
              <ConfirmRemove
                title={`Unlink ${unlinking}?`}
                label="Unlink"
                cancel={() => setUnlinking(null)}
                go={() => {
                  const r = unlinking;
                  setUnlinking(null);
                  void act(() => call("DELETE", `${base}/${r}`));
                }}
              >
                <p>The project stops reading it at once, and what was read of it, kept bodies included, goes within a day.</p>
              </ConfirmRemove>
            )}
          </Section>
          {boards !== null && boards !== "off" && <PlaneConnections p={p} />}
          {boards !== null && boards !== "off" && <BoardSettings p={p} />}
          <IssueHomeSetting key={linked} p={p} />
          <Environments p={p} />
          <RunGrant project={p.id} />
          <WorkspaceSize p={p} />
          <FinishStep p={p} />
          <AgentModel p={p} />
          <ProjectAgents p={p} />
          <ClaudeSpend p={p} />
          <ClientApps p={p} />
          <TeamChat p={p} />
        </div>
      </div>
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
