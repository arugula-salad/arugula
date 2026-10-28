// The platform page (M150), at each route: the bar, then the page. Chat
// draws its own (the rooms view puts its search in the bar), in the same
// frame (./pages/chat, UI audit #169). Before anyone
// is signed in here it says where to sign in.

import { useEffect, useState } from "preact/hooks";
import { chat } from "../shared/chat-store";
import { useSubscribe } from "../shared/ui/hooks";
import { MaintenanceNote } from "../shared/ui/maintenance-note";
import { PageHeader } from "../shared/ui/page-header";
import { PlatformBar } from "./bar";
import { names, resolve, roomElsewhere } from "./links";
import { go, pathOf, useRoute, type Route } from "./router";
import { controlOrigin, session } from "./session";
import { signIn } from "./signin";
import { toasts } from "./toast";
import { IssuePage } from "./pages/issue";
import { IssuesPage, NewIssuePage } from "./pages/issues";
import { issues } from "../shared/issues/store";
import { ChatPage } from "./pages/chat";
import { Home } from "./pages/home";
import { DeployPage } from "./pages/deploy";
import { NewDeployPage } from "./pages/new-deploy";
import { DeploysPage } from "./pages/deploys";
import { InboxPage } from "./pages/inbox";
import { Missing, ProjectHome } from "./pages/project";
import { Installed, ProjectSettings } from "./pages/project-settings";
import { SettingsPage } from "./pages/settings";
import { HostedWorkspacePage, HostedWorkspaces } from "./pages/hosted-workspace";
import { RunPage, RunsPage } from "./pages/runs";
import { BoardPage, BoardsPage } from "./pages/boards";
import { TriagePage } from "./pages/triage";
import { DocPage, DocsPage } from "./pages/docs";
import { projectDocs } from "../shared/docs/store";
import { SessionPage } from "./pages/session";
import { runs } from "./runs";
import { runTitle } from "./runs-text";
import { titleOf } from "./titles";
import { barProject, where } from "./where";

export function PlatformPage() {
  useSubscribe((fn) => session.subscribe(fn));
  const r = useRoute();
  useEffect(() => void session.load(), []);
  useSlug(r);
  const lost = useFind(r);
  useTitle(r);
  const s = session.state;
  if (s === null) return <Shell><p class="platform-section dim">Loading…</p></Shell>;
  if (s === "signed-out") return <SignIn />;
  if (typeof s === "object") return <Shell><p class="platform-section control-error">{s.error}</p></Shell>;
  if (r.page === "chat") return <ChatPage project={r.project} route={{ room: r.room, msg: r.msg, thread: r.thread, view: r.view, q: r.q }}><Toast /></ChatPage>;
  if (r.page === "dm") return <ChatPage project={r.project} route={{ room: r.room, msg: r.msg, thread: r.thread }}><Toast /></ChatPage>;
  return <Shell>{body(r, lost)}</Shell>;
}

/** `lost`: the issue or run an id-only address names, when it couldn't be
 * read. */
function body(r: Route, lost: string | null) {
  switch (r.page) {
    case "find":
      if (r.what === "issue" && lost === r.issue) return <Missing what="That issue isn't one you can read (or it's gone)." />;
      if (r.what === "run" && lost === r.run) return <Missing what="That run isn't one you can read (or it's gone)." />;
      return <p class="platform-section dim">Finding it…</p>;
    case "home":
      return <Home />;
    case "project":
      return <ProjectHome id={r.project} />;
    case "issues":
      return <IssuesPage project={r.project} filter={{ state: r.state, q: r.q, label: r.label, assignee: r.assignee }} />;
    case "new-issue":
      return <NewIssuePage project={r.project} />;
    case "issue":
      return <IssuePage project={r.project} issue={r.issue} msg={r.msg} />;
    case "triage":
      return <TriagePage project={r.project} />;
    case "boards":
      return <BoardsPage project={r.project} />;
    case "board":
      return <BoardPage project={r.project} board={r.board} />;
    case "docs":
      return <DocsPage project={r.project} />;
    case "doc":
      return <DocPage project={r.project} doc={r.doc} />;
    case "runs":
      return <RunsPage project={r.project} filter={{ state: r.state, trigger: r.trigger, spec: r.spec }} />;
    case "run":
      return <RunPage project={r.project} run={r.run} msg={r.msg} />;
    case "session":
      return <SessionPage project={r.project} session={r.session} />;
    case "workspaces":
      return <HostedWorkspaces project={r.project} />;
    case "project-settings":
      return <ProjectSettings id={r.project} link={r.link} />;
    case "deploys":
      return <DeploysPage project={r.project} />;
    case "new-deploy":
      return <NewDeployPage project={r.project} env={r.env} />;
    case "installed":
      return <Installed project={r.project} repo={r.repo} />;
    case "workspace":
      // M184: a hosted workspace is a machine of the project.
      return <HostedWorkspacePage project={r.project} workspace={r.workspace} />;
    case "inbox":
      return <InboxPage />;
    case "settings":
      return <SettingsPage />;
    case "deploy":
      return <DeployPage id={r.deploy} />;
    default:
      return <Missing what="Nothing's at this address." />;
  }
}

function Shell({ children }: { children: preact.ComponentChildren }) {
  return (
    <div class="platform" data-platform>
      <PlatformBar />
      <MaintenanceNote />
      <main class="platform-main">{children}</main>
      <Toast />
    </div>
  );
}

function SignIn() {
  return (
    <div class="platform" data-platform data-platform-signed-out>
      <main class="platform-main">
        <section class="platform-section sw-page">
          <PageHeader title="Arugula" />
          <p>
            <button class="primary" data-sign-in onClick={() => void signIn(controlOrigin())}>
              Sign in
            </button>{" "}
            <span class="dim">on Control, where your passkeys are; it brings you back here.</span>
          </p>
        </section>
      </main>
    </div>
  );
}

function Toast() {
  useSubscribe((fn) => toasts.subscribe(fn));
  const t = toasts.shown();
  return t ? (
    <div class="platform-toast" role="status" data-platform-toast>
      {t.text}
    </div>
  ) : null;
}

/** An id-only address (`/chat/:room`…) goes where it belongs as soon as
 * the page can say, in its place in history. */
function useFind(r: Route): string | null {
  useSubscribe((fn) => chat.subscribe(fn));
  useSubscribe((fn) => issues.subscribe(fn));
  useSubscribe((fn) => runs.subscribe(fn));
  const to = r.page === "find" && session.state === "ready" ? resolve(r) : null;
  // An issue's or a run's project is its own to say: read it once.
  const ask = r.page === "find" && (r.what === "issue" || r.what === "run") && session.state === "ready" && !to ? (r.what === "issue" ? r.issue : r.run) : null;
  // The one that couldn't be read, while the address names it: going
  // anywhere else and back asks again.
  const [lost, setLost] = useState<string | null>(null);
  useEffect(() => {
    setLost(null);
    let live = true;
    if (ask) (ask.startsWith("run_") ? runs.get(ask) : issues.get(ask)).catch(() => live && setLost(ask));
    return () => void (live = false);
  }, [ask]);
  useEffect(() => {
    if (to) go(to, true);
  }, [to]);
  return lost;
}

/** The tab's title for the route (UI audit #159), as names arrive. */
function useTitle(r: Route) {
  useSubscribe((fn) => where.subscribe(fn));
  const title = titleOf(r, {
    project: (id) => session.project(id)?.name,
    issue: (id) => issues.known(id),
    doc: (id) => projectDocs.title(id),
    run: (id) => {
      const x = runs.known(id);
      return x ? runTitle(x.op) : null;
    },
    room: (id) => {
      const x = chat.room(id);
      if (!x) return null;
      if (x.kind === "channel") return `#${x.name}`;
      // A DM by its people, unless it was renamed.
      if (x.kind === "dm") return x.name || x.others.map((o) => o.name).join(", ") || x.with?.name;
      return x.name;
    },
    shown: barProject(r),
  });
  useEffect(() => void (document.title = title), [title]);
}

/** Put the address's slugs right once the names are known: only the ids
 * are read, so an old slug lands, and the bar shows the current one. A
 * room of another project's goes to its own (UI audit #169). */
function useSlug(r: Route) {
  const want = r.page === "missing" || r.page === "find" ? null : (roomElsewhere(r) ?? pathOf(r, names));
  useEffect(() => {
    if (want && want !== location.pathname + location.search && session.state === "ready") go(want, true);
  }, [want, session.state]);
}
