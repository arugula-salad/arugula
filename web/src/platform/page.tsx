// The platform page (M150), at each route: the bar, then the page. Chat
// draws its own (the rooms view puts its search in the bar), in the same
// frame (./pages/chat, UI audit #169). Before anyone
// is signed in here it says where to sign in.

import { useEffect, useState } from "preact/hooks";
import { chat } from "../shared/chat-store";
import { useSubscribe } from "../shared/ui/hooks";
import { usePlatformApi } from "../shared/platform-api";
import { MaintenanceNote } from "../shared/ui/maintenance-note";
import { PageGuard } from "../shared/ui/guard";
import { PageHeader } from "../shared/ui/page-header";
import { PlatformBar } from "./bar";
import { PlaceViews, WithRail } from "./rail";
import { homes, movedHome, names, resolve, roomElsewhere } from "./links";
import { currentProject, entryPath, type Entry } from "./current-project";
import { slugIn } from "./paths";
import { arrive, openingChat } from "./chat-nav";
import { current, go, onRoute, pathOf, popCount, pushCount, useRoute, type Route } from "./router";
import { installStack } from "../shared/kit/frame/stack";
import type { MachineAt } from "../shared/machine-paths";
import { controlOrigin, session } from "./session";
import { signIn } from "./signin";
import { toasts } from "./toast";
import { IssuePage } from "./pages/issue";
import { issues } from "../shared/issues/store";
import { ChatPage } from "./pages/chat";
import { Home } from "./pages/home";
import { DeployPage } from "./pages/deploy";
import { NewDeployPage } from "./pages/new-deploy";
import { DeploysPage } from "./pages/deploys";
import { InboxPage } from "./pages/inbox";
import { Missing, ProjectHome } from "./pages/project";
import { Installed } from "./pages/project-settings";
import { MovePage } from "./move/move-page";
import { SplitNote } from "./move/split-note";
import { DesignPage } from "./pages/design";
import { SettingsPage } from "./pages/settings";
import { HostedWorkspacePage, HostedWorkspaces } from "./pages/hosted-workspace";
import { RunPage, RunsPage } from "./pages/runs";
import { AllBoardsPage, BoardsPage } from "./pages/boards";
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
import { KeysSheetLayer } from "../shared/kit/frame/keys-sheet";
import { addGoSource, GoToLayer } from "../shared/kit/frame/goto";
import { platformGoTo } from "./goto-source";
import { NewProjectLayer } from "./new-project";

export function PlatformPage() {
  // Go to…: your projects, their places, boards and Docs.
  useEffect(() => addGoSource(platformGoTo), []);
  // One back stack across projects; Escape's last use and flip back.
  useEffect(() => installStack({ here: () => ({ path: location.pathname + location.search, project: barProject(current()) ?? null }), go: (path) => go(path), onRoute, pops: popCount }), []);
  // The design reference needs no one signed in and no project, so nothing of
  // the session is loaded for it (#693).
  return (
    <>
      {useRoute().page === "design" ? (
        <PageGuard at="design">
          <DesignPage />
        </PageGuard>
      ) : (
        <Routed />
      )}
      {/* `?`: every key the page has; ⌘K: a place, anywhere, or a command. */}
      <KeysSheetLayer />
      <GoToLayer />
      <NewProjectLayer />
    </>
  );
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
  // Each page in a guard (Arugula #615): one that throws while drawing says
  // so in its place, and the rail goes on working. Its address without
  // slugs: putting a slug right isn't another page.
  const at = pathOf(r);
  if (s === null) return <Shell><p class="platform-section dim">Loading…</p></Shell>;
  if (s === "signed-out") return <SignIn />;
  if (typeof s === "object") return <Shell><p class="platform-section control-error">{s.error}</p></Shell>;
  // Chat draws its own frame, so a failure is drawn in the others'.
  const inShell = (note: preact.ComponentChildren) => <Shell>{note}</Shell>;
  if (r.page === "chat")
    return (
      <PageGuard at={at} frame={inShell}>
        <ChatPage project={r.project} route={{ room: r.room, msg: r.msg, thread: r.thread, view: r.view, q: r.q, all: r.all }}><Toast /></ChatPage>
      </PageGuard>
    );
  if (r.page === "dm")
    return (
      <PageGuard at={at} frame={inShell}>
        <ChatPage project={r.project} route={{ room: r.room, msg: r.msg, thread: r.thread }}><Toast /></ChatPage>
      </PageGuard>
    );
  // A machine's page has its own header (Arugula #989): the rail, then it.
  if (r.page === "machines" && Machines) return <MachinesShell project={r.project} at={r.at} page={at} />;
  return <Shell page={at}>{body(r, lost)}</Shell>;
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
      return <ProjectHome id={r.project} board={r.board} card={r.card} zoom={r.zoom} />;
    case "issues":
      return <BoardsPage project={r.project} issues={{ state: r.state, q: r.q, label: r.label, assignee: r.assignee }} />;
    case "new-issue":
      // New issue is a sheet over All issues (Lift 5a).
      return <BoardsPage project={r.project} issues={{}} adding />;
    case "issue":
      return <IssuePage project={r.project} issue={r.issue} msg={r.msg} thread={r.thread} />;
    case "triage":
      return <BoardsPage project={r.project} triage />;
    case "boards":
      return <BoardsPage project={r.project} />;
    case "board":
      return <BoardsPage project={r.project} board={r.board} />;
    case "docs":
      return <DocsPage project={r.project} all={r.all} shared={r.shared} />;
    case "doc":
      return <DocPage project={r.project} doc={r.doc} />;
    case "runs":
      return <RunsPage project={r.project} filter={{ state: r.state, trigger: r.trigger, spec: r.spec }} />;
    case "run":
      return <RunPage project={r.project} run={r.run} msg={r.msg} thread={r.thread} />;
    case "sessions":
      // Folded into each card's row (Arugula #1082): useSlug takes an old
      // link on to Boards.
      return null;
    case "session":
      return <SessionPage project={r.project} session={r.session} />;
    case "workspaces":
      return <HostedWorkspaces project={r.project} />;
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
      return <AllBoardsPage board={r.board} />;
    case "settings":
      return <SettingsPage r={r} />;
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

/** `page`: the page's address, for its guard. */
function Shell({ page, children }: { page?: string; children: preact.ComponentChildren }) {
  return (
    <div class="platform" data-platform>
      <WithRail>
        <PlatformBar>
          <PlaceViews />
        </PlatformBar>
        <MaintenanceNote />
        <SplitNote />
        <main class="platform-main">
          <PageGuard at={page}>{children}</PageGuard>
        </main>
      </WithRail>
      <Toast />
    </div>
  );
}

/** Machines, beside the rail. The list has the bar, with Machines'
 * views (Arugula #1345); a machine's page and Swarm draw their own heads,
 * and on a phone keep the bottom (a pane's key bar, Swarm's field). */
function MachinesShell({ project, at, page }: { project: string; at: MachineAt; page: string }) {
  // The lists (the machines, their panes) and Swarm, the Overview's view
  // (Arugula #1348), sit under the platform's bar; a machine's page has its own.
  const barred = (at.at === "machines" && at.pane === undefined && !at.name) || at.at === "panes" || at.at === "swarm";
  return (
    <div class="platform platform-machines" data-platform data-platform-machines>
      <WithRail tabs={barred}>
        {barred && (
          <PlatformBar>
            <PlaceViews />
          </PlatformBar>
        )}
        <PageGuard at={page}>
          <MachinesPage project={project} at={at} />
        </PageGuard>
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
    <div class={t.action ? "platform-toast split-note" : "platform-toast"} role="status" data-platform-toast>
      <span>{t.text}</span>
      {t.action && (
        <button type="button" class="link" data-toast-action onClick={() => toasts.act()}>
          {t.action.label}
        </button>
      )}
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
  // Not after a Back: an effect runs after the paint, and a Back before
  // then has left the page it was found for (Arugula #1074).
  const pops = popCount();
  useEffect(() => {
    if (to && popCount() === pops) arrive(to);
  }, [to, pops]);
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

/** Where a page that moved is now: the Sessions page folded into Boards
 * (Arugula #1082), Build's panes into Machines (#1346), every project's
 * Boards into a project's (Lift 5a). Here, not in an effect of its own, which raced this one:
 * the slug's rewrite ran after it and put the old address back (Arugula
 * #1155). */
function movedPage(r: Route): Route {
  if (r.page === "sessions") return { page: "boards", project: r.project };
  // Every project's Boards (`/boards`, `?all=1`) is the current project's, else your first's (Lift 5a, Arugula #1525).
  if (r.page === "all-boards" && !r.board) {
    const here = currentProject();
    const project = here && session.project(here) ? here : session.projects[0]?.id;
    if (project) return { page: "boards", project };
  }
  // Build's list of panes (#1083) is the Machines page (Arugula #1346).
  if (r.page === "machines" && r.at.at === "panes") return { ...r, at: { at: "machines", ...(r.at.agents ? { agents: true } : {}) } };
  return r;
}

/** Put the address's slugs right once the names are known: only the ids
 * are read, so an old slug lands, and the bar shows the current one. An id
 * whose name isn't known yet (a Doc's, until it's read) keeps the slug it
 * has, so loading a page doesn't take it off the address and put it back. A
 * room of another project's goes to its own (UI audit #169), and a page that
 * moved to its new address. The hash stays: it can name a Doc's tab. */
function useSlug(r: Route) {
  // An issue, Doc or board moved by a split goes to its new project's address (Arugula #972).
  useSubscribe((fn) => homes.subscribe(fn));
  useSubscribe((fn) => issues.subscribe(fn));
  const known = (id: string) => names(id) ?? slugIn(location.pathname, id);
  const want = r.page === "missing" || r.page === "find" || (r.page === "home" && r.to) ? null : (roomElsewhere(r) ?? movedHome(r) ?? pathOf(movedPage(r), known));
  // Not after a Back, nor after a step to elsewhere: the effect runs after
  // the paint, and a Back (or Escape's step to the Overview) before then
  // would have it replace the entry gone to with this page's (Arugula #1074).
  const pops = popCount();
  const pushes = pushCount();
  useEffect(() => {
    // Settings' `#section` is in the path now (Arugula #1104): the hash goes.
    if (want && want !== location.pathname + location.search && popCount() === pops && pushCount() === pushes && session.state === "ready") go(want + (r.page === "settings" ? "" : location.hash), true);
  }, [want, pops, session.state]);
}
