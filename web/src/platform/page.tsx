// The platform page (M150), at each route: the bar, then the page. Chat
// draws its own (the rooms view puts its search in the bar), in the same
// frame (./pages/chat, UI audit #169). Before anyone
// is signed in here it says where to sign in.

import { useEffect, useState } from "preact/hooks";
import { chat } from "../shared/chat-store";
import { useSubscribe } from "../shared/ui/hooks";
import { usePlatformApi } from "../shared/platform-api";
import { MaintenanceNote } from "../shared/ui/maintenance-note";
import { PageHeader } from "../shared/ui/page-header";
import { PlatformBar } from "./bar";
import { WithRail } from "./rail";
import { names, resolve, roomElsewhere } from "./links";
import { currentProject, entryPath, type Entry } from "./current-project";
import { slugIn } from "./paths";
import { arrive, openingChat } from "./chat-nav";
import { follow, go, pathOf, useRoute, type Route } from "./router";
import type { MachineAt } from "../shared/machine-paths";
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
import { MovePage } from "./move/move-page";
import { DesignPage } from "./pages/design";
import { SettingsPage } from "./pages/settings";
import { OwnerSettings } from "./pages/owner-settings";
import { HostedWorkspacePage, HostedWorkspaces } from "./pages/hosted-workspace";
import { RunPage, RunsPage } from "./pages/runs";
import { SessionsPage } from "./pages/sessions";
import { AllBoardsPage, BoardPage, BoardsPage } from "./pages/boards";
import { DocPage, DocsPage } from "./pages/docs";
import { projectDocs } from "../shared/docs/store";
import { SessionPage } from "./pages/session";
import { TeamPage } from "./pages/team";
import { MachinesPage } from "./pages/machines";
import { runs } from "./runs";
import { runTitle } from "./runs-text";
import { titleOf } from "./titles";
import { lastProject, notePlace, placeOf } from "./places-kept";
import { seenReady, subscribeSeen } from "../shared/seen";
import { barProject, where } from "./where";

export function PlatformPage() {
  // The design reference needs no one signed in and no project, so nothing of
  // the session is loaded for it (#693).
  return useRoute().page === "design" ? <DesignPage /> : <Routed />;
}

function Routed() {
  useSubscribe((fn) => session.subscribe(fn));
  const { Machines } = usePlatformApi();
  const r = useRoute();
  useEffect(() => void session.load(), []);
  useSlug(r);
  const lost = useFind(r);
  useTitle(r);
  useWhereIWas(r);
  const s = session.state;
  if (s === null) return <Shell><p class="platform-section dim">Loading…</p></Shell>;
  if (s === "signed-out") return <SignIn />;
  if (typeof s === "object") return <Shell><p class="platform-section control-error">{s.error}</p></Shell>;
  if (r.page === "chat") return <ChatPage project={r.project} route={{ room: r.room, msg: r.msg, thread: r.thread, view: r.view, q: r.q, all: r.all }}><Toast /></ChatPage>;
  if (r.page === "dm") return <ChatPage project={r.project} route={{ room: r.room, msg: r.msg, thread: r.thread }}><Toast /></ChatPage>;
  // A machine's page has its own header (Arugula #989): the rail, then it.
  if (r.page === "machines" && Machines) return <MachinesShell project={r.project} at={r.at} />;
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
      return r.to ? <Entering to={r.to} /> : <Home />;
    case "project":
      return <ProjectHome id={r.project} board={r.board} card={r.card} />;
    case "issues":
      return <IssuesPage project={r.project} filter={{ state: r.state, q: r.q, label: r.label, assignee: r.assignee }} />;
    case "new-issue":
      return <NewIssuePage project={r.project} />;
    case "issue":
      return <IssuePage project={r.project} issue={r.issue} msg={r.msg} />;
    case "triage":
      return <BoardsPage project={r.project} scroll />;
    case "boards":
      return <BoardsPage project={r.project} />;
    case "board":
      return <BoardPage project={r.project} board={r.board} />;
    case "docs":
      return <DocsPage project={r.project} all={r.all} />;
    case "doc":
      return <DocPage project={r.project} doc={r.doc} />;
    case "runs":
      return <RunsPage project={r.project} filter={{ state: r.state, trigger: r.trigger, spec: r.spec }} />;
    case "run":
      return <RunPage project={r.project} run={r.run} msg={r.msg} />;
    case "sessions":
      return <SessionsPage project={r.project} filter={{ label: r.label, tracker: r.tracker, asker: r.asker, going: r.going }} />;
    case "session":
      return <SessionPage project={r.project} session={r.session} />;
    case "workspaces":
      return <HostedWorkspaces project={r.project} />;
    case "project-settings":
      return <ProjectSettings id={r.project} link={r.link} />;
    case "move":
      return <MovePage project={r.project} />;
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
      return <InboxPage all={r.all} />;
    case "all-boards":
      return <AllBoardsPage all={r.all} />;
    case "settings":
      return <SettingsPage />;
    case "owner-settings":
      return <OwnerSettings owner={r.owner} />;
    case "deploy":
      return <DeployPage id={r.deploy} />;
    case "team":
      return <TeamPage team={r.team} tab={r.tab} />;
    case "machines":
      return <MachinesPage project={r.project} at={r.at} />;
    default:
      return <Missing what="Nothing's at this address." />;
  }
}

/** A shell button's place (`/?to=docs`): in the current project once the page
 * knows the reader's, else the global page. */
function Entering({ to }: { to: Entry }) {
  useSubscribe((fn) => session.subscribe(fn));
  const ready = session.state === "ready";
  useEffect(() => {
    if (!ready) return;
    // The shell's Chat button hands over by a page load: still opened from the app.
    if (to === "chat") openingChat();
    go(entryPath(to, session.project(currentProject()), names), true);
  }, [ready]);
  return <p class="platform-section dim">Finding it…</p>;
}

function Shell({ children }: { children: preact.ComponentChildren }) {
  return (
    <div class="platform" data-platform>
      <WithRail>
        <PlatformBar />
        <MaintenanceNote />
        <main class="platform-main">{children}</main>
      </WithRail>
      <Toast />
    </div>
  );
}

/** The machines' places, beside the rail. The list has the bar, with
 * Panes and Swarm (decision 3); a machine's page and Swarm draw their own
 * heads, and on a phone keep the bottom (a pane's key bar, Swarm's field). */
function MachinesShell({ project, at }: { project: string; at: MachineAt }) {
  const list = at.at === "machines" && at.pane === undefined && !at.name;
  const swarmTo = pathOf({ page: "machines", project, at: { at: "swarm" } }, names);
  return (
    <div class="platform platform-machines" data-platform data-platform-machines>
      <WithRail tabs={list}>
        {list && (
          <PlatformBar>
            <nav class="places" aria-label="Machines' places" data-machines-places>
              <span class="place frame-place here" aria-current="page" data-machines-panes>
                Panes
              </span>
              <a class="place frame-place" href={swarmTo} data-machines-swarm onClick={(e) => follow(e, swarmTo)}>
                Swarm
              </a>
            </nav>
          </PlatformBar>
        )}
        <MachinesPage project={project} at={at} />
      </WithRail>
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
            <span class="dim">on your machines page, where your passkeys are; it brings you back here.</span>
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
    if (to) arrive(to);
  }, [to]);
  return lost;
}

/** The app opened at `/` (not a step to it inside the page): it goes back
 * to where you were in your last project (Arugula #989, decision 4). */
let reopening = location.pathname === "/" && location.search === "";

/** Keep where you are, per project; and on opening the app, go back there
 * once your prefs and projects have loaded. A project you've never opened
 * a page of opens on its Overview; with no project kept, the list stays. */
function useWhereIWas(r: Route) {
  useSubscribe(subscribeSeen);
  const ready = session.state === "ready";
  const path = location.pathname + location.search;
  useEffect(() => {
    if (r.page !== "home") reopening = false;
    notePlace(r, path);
  }, [path]);
  useEffect(() => {
    if (!reopening || r.page !== "home" || r.to || !ready || !seenReady()) return;
    reopening = false;
    const project = lastProject();
    if (!project || !session.project(project)) return;
    go(placeOf(project) ?? pathOf({ page: "project", project }, names), true);
  }, [r.page, ready, seenReady()]);
}

/** The tab's title for the route (UI audit #159), as names arrive. */
function useTitle(r: Route) {
  useSubscribe((fn) => where.subscribe(fn));
  // A Doc renamed here or by someone else reaches its tab (#979).
  useSubscribe((fn) => projectDocs.subscribe(fn));
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
    team: names,
    shown: barProject(r),
  });
  useEffect(() => void (document.title = title), [title]);
}

/** Put the address's slugs right once the names are known: only the ids
 * are read, so an old slug lands, and the bar shows the current one. An id
 * whose name isn't known yet (a Doc's, until it's read) keeps the slug it
 * has, so loading a page doesn't take it off the address and put it back. A
 * room of another project's goes to its own (UI audit #169). The hash stays:
 * it can name a Doc's tab. */
function useSlug(r: Route) {
  const known = (id: string) => names(id) ?? slugIn(location.pathname, id);
  const want = r.page === "missing" || r.page === "find" || (r.page === "home" && r.to) ? null : (roomElsewhere(r) ?? pathOf(r, known));
  useEffect(() => {
    if (want && want !== location.pathname + location.search && session.state === "ready") go(want + location.hash, true);
  }, [want, session.state]);
}
