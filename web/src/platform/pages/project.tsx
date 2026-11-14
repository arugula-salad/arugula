// `/p/:project`: a project's home (M150): its channels and direct
// messages, its issues, its last runs, its cloud workspaces and its agent
// sessions in numbers (M265), its drafts waiting on linked repos (M170),
// and who's in its team. Membership shows read only: rosters change in the
// shell, signed by a device ("The platform page is on app.arugula.io, not
// on the keys' origin"), so the link goes there. A section with nothing
// in it says what it's for and offers the next step as a button
// (Discovery 5): Link a repo, Make an environment, Start a run, Create a
// cloud workspace, Turn on chat or Ask an owner.

import { Card } from "../../shared/ui/card";
import { useEffect, useState } from "preact/hooks";
import { chat } from "../../shared/chat-store";
import { ProjectDrafts } from "../drafts";
import { MsgAvatar } from "../../shared/chat/avatar";
import { Time } from "../../shared/ui/time";
import { EmptyLine, EmptyState } from "../../shared/ui/block-states";
import { useSubscribe } from "../../shared/ui/hooks";
import { PageHeader } from "../../shared/ui/page-header";
import { names, roomPath } from "../links";
import { follow, pathOf } from "../router";
import { runs, type RunView } from "../runs";
import { STATES, runTitle } from "../runs-text";
import { SectionHead } from "../../shared/ui/section-head";
import { controlOrigin, session, type PlatformProject } from "../session";
import { roomTone, rowTone, runTone } from "../swarm";
import { HostedWorkspaceList, NewWorkspaceButton, NoWorkspaces, useHostedWorkspaces } from "./hosted-workspace";
import { whyNoStart } from "./runs";
import { platformFetch } from "../../shared/maintenance";
import type { LinkedRepos } from "../../shared/proto.gen";
import { toast } from "../toast";
import { ProjectBoards, useBoards } from "./boards";
import { ProjectDocs } from "./docs";
import { ProjectIssues } from "./issues";
import { SessionNumbers } from "../session-stats";
import { Count } from "../../shared/ui/state-chip";
import { usePlatformApi } from "../../shared/platform-api";

const ROLE: Record<string, string> = { owner: "Owner", editor: "Editor", viewer: "Viewer" };

export function ProjectHome({ id, board, card }: { id: string; board?: string; card?: string }) {
  const { Overview } = usePlatformApi();
  useSubscribe((fn) => session.subscribe(fn));
  useSubscribe((fn) => chat.subscribe(fn));
  const workspaces = useHostedWorkspaces(id);
  const boards = useBoards(id);
  const p = session.project(id);
  if (!p) return <Missing what="That project isn't one of yours (or it's gone)." />;
  const rooms = chat.on ? chat.rooms.filter((r) => (p.team ? r.team === p.team.id : !r.team) && (r.kind === "channel" || r.kind === "dm")) : [];
  const chatPath = pathOf({ page: "chat", project: p.id }, names);
  const settingsPath = pathOf({ page: "project-settings", project: p.id }, names);
  const workspacesPath = pathOf({ page: "workspaces", project: p.id }, names);
  const sections = <HomeSections p={p} boards={boards} workspaces={workspaces} rooms={rooms} chatPath={chatPath} workspacesPath={workspacesPath} />;
  return (
    <section class="platform-section sw-page" data-project-home={p.id}>
      <PageHeader
        title={p.name}
        named
        sub={
          <a href={settingsPath} data-project-settings onClick={(e) => follow(e, settingsPath)}>
            Project settings
          </a>
        }
      />
      {Overview ? (
        <Overview project={p.id} board={board} card={card}>
          {sections}
        </Overview>
      ) : (
        sections
      )}
    </section>
  );
}

/** What the project's page showed before the Overview, below it now: its
 * drafts, chat, boards, issues, Docs, runs, repos, environments, cloud
 * workspaces and people. */
function HomeSections({ p, boards, workspaces, rooms, chatPath, workspacesPath }: { p: PlatformProject; boards: ReturnType<typeof useBoards>; workspaces: ReturnType<typeof useHostedWorkspaces>; rooms: typeof chat.rooms; chatPath: string; workspacesPath: string }) {
  const boardsOn = boards !== null;
  const nameOf = (account: string) =>
    account === session.me?.account ? "you" : (p.team?.members.find((m) => m.account === account)?.name ?? "someone who left");
  return (
      /* A project of one has no members beside it: its lists take the
          page's width (UI audit #162). */
      <div class={p.team ? "platform-columns" : "platform-columns solo"}>
        <div>
          <ProjectDrafts project={p.id} nameOf={nameOf} />
          <SectionHead title="Chat" all={chatPath} onAll={(e) => follow(e, chatPath)} />
          {chat.on === false && chat.signedOut && <p class="dim">You're signed out of Arugula cloud: reload the page to sign in again.</p>}
          {chat.on === false && !chat.signedOut && <ChatOff p={p} />}
          {chat.on && rooms.length === 0 && <EmptyLine>No channels yet.</EmptyLine>}
          <ul class="platform-list card-list" data-project-rooms>
            {rooms
              .sort((a, b) => Number(a.kind === "dm") - Number(b.kind === "dm") || a.name.localeCompare(b.name))
              .map((r) => {
                const to = roomPath(r.id);
                return (
                  <Card as="li" dense key={r.id} tone={rowTone(roomTone(r.unread, r.mentions))}>
                    <a href={to} data-project-room={r.id} onClick={(e) => follow(e, to)}>
                      {r.kind === "channel" ? `# ${r.name}` : `@ ${r.with?.name ?? "someone who left"}`}
                      {r.unread > 0 && <Count for={r.mentions ? "mention" : "you"} class="count-after">{r.unread}</Count>}
                    </a>
                  </Card>
                );
              })}
          </ul>
          {/* With boards on, they lead; the issues on no board are on the Boards page (#656). */}
          {boardsOn && <ProjectBoards p={p} list={boards} />}
          <ProjectIssues p={p} />
          <ProjectDocs p={p} />
          <ProjectRuns p={p} workspaces={workspaces} />
          <ProjectSetup p={p} />
          {!boardsOn && <ProjectBoards p={p} list={boards} />}
          {(session.hosted || !!workspaces?.length) && (
            <>
              <SectionHead title="Cloud workspaces" all={workspacesPath} onAll={(e) => follow(e, workspacesPath)} />
              {workspaces === null ? null : workspaces.length ? <HostedWorkspaceList project={p.id} list={workspaces} /> : <NoWorkspaces project={p.id} />}
              {!!workspaces?.length && <SessionNumbers project={p.id} />}
            </>
          )}
        </div>
        {p.team && <Members p={p} />}
      </div>
  );
}

/** Chat is off: Turn on chat for a team's owner (Teams, on Control, where
 * a change to the team is signed), Ask an owner for anyone else (a note
 * to send them, copied), and for a personal project, where it comes from. */
function ChatOff({ p }: { p: PlatformProject }) {
  const teams = `${controlOrigin()}/#teams`;
  if (!p.team)
    return (
      <EmptyState
        where="page"
        data-chat-off
        title="Chat comes with a team."
        why="A team has channels and direct messages. Make a team, or turn chat on for one you own, under Teams on your machines page."
        action={
          <a class="secondary" href={teams} data-chat-teams>
            Teams on your machines page
          </a>
        }
      />
    );
  if (p.team.role === "owner")
    return (
      <EmptyState
        where="page"
        data-chat-off
        title={`Chat is off for ${p.team.name}.`}
        why="Turned on, the team gets channels and direct messages, here and on your machines page."
        action={
          <a class="primary" href={teams} data-chat-turn-on>
            Turn on chat
          </a>
        }
      />
    );
  const owners = p.team.members.filter((m) => m.role === "owner").map((m) => m.name);
  const ask = async () => {
    const note = `Could you turn on chat for ${p.team!.name}? It's under Teams on your machines page: ${teams}`;
    try {
      await navigator.clipboard.writeText(note);
      toast(`Copied a note asking for it: send it to ${owners[0] ?? "an owner"}.`);
    } catch {
      toast(note);
    }
  };
  return (
    <EmptyState
      where="page"
      data-chat-off
      title={`Chat is off for ${p.team.name}.`}
      why={`Its owners turn it on${owners.length ? ` (${owners.join(", ")})` : ""}.`}
      action={
        <button class="secondary" data-chat-ask-owner onClick={() => void ask()}>
          Ask an owner
        </button>
      }
    />
  );
}

/** The project's linked repos and environments, by name, so its home says
 * what it's linked to and where it deploys; empty, what each is for and
 * the button that makes one (the project's settings, at its section). */
function ProjectSetup({ p }: { p: PlatformProject }) {
  const [repos, setRepos] = useState<string[] | null>(null);
  const [envs, setEnvs] = useState<string[] | null>(null);
  useEffect(() => {
    let live = true;
    const base = `/api/projects/${encodeURIComponent(p.id)}`;
    const read = <T,>(path: string, f: (v: T) => string[], set: (v: string[] | null) => void) =>
      platformFetch(`${base}/${path}`, { credentials: "same-origin" })
        .then((r) => (r.ok ? (r.json() as Promise<T>) : null))
        .then((v) => live && set(v ? f(v) : null))
        .catch(() => live && set(null));
    void read<LinkedRepos>("repos", (v) => v.repos.filter((r) => r.purge_after === null).map((r) => r.full_name), setRepos);
    void read<{ environments: { name: string }[] }>("environments", (v) => v.environments.map((e) => e.name), setEnvs);
    return () => void (live = false);
  }, [p.id]);
  const settings = pathOf({ page: "project-settings", project: p.id }, names);
  const changes = !p.team || p.team.role !== "viewer";
  const owner = !p.team || p.team.role === "owner";
  const step = (to: string, label: string, data: string) => (
    <a class="primary" href={to} onClick={(e) => follow(e, to)} {...{ [`data-${data}`]: "" }}>
      {label}
    </a>
  );
  const rows = (list: string[], data: string) => (
    <ul class="platform-list card-list" {...{ [`data-${data}`]: "" }}>
      {list.map((n) => (
        <Card as="li" dense key={n}>
          <span class="sw-mono">{n}</span>
        </Card>
      ))}
    </ul>
  );
  return (
    <>
      {repos !== null && (
        <>
          <SectionHead title="Repos" all={`${settings}#repos`} onAll={(e) => follow(e, `${settings}#repos`)} />
          {repos.length ? (
            rows(repos, "project-repos")
          ) : (
            <EmptyState
              where="page"
              data-project-repos-none
              title="No repos linked yet."
              why={changes ? REPOS_MEAN : `${REPOS_MEAN} A team's owners and editors link repos.`}
              action={changes && step(`${settings}#repos`, "Link a repo", "project-link-repo")}
            />
          )}
        </>
      )}
      {envs !== null && (
        <>
          <SectionHead title="Environments" all={`${settings}#environments`} onAll={(e) => follow(e, `${settings}#environments`)} />
          {envs.length ? (
            rows(envs, "project-envs")
          ) : (
            <EmptyState
              where="page"
              data-project-envs-none
              title="No environments yet."
              why={owner ? ENVIRONMENTS_MEAN : `${ENVIRONMENTS_MEAN} A project's owners make its environments.`}
              action={owner && step(`${settings}#environments`, "Make an environment", "project-make-env")}
            />
          )}
        </>
      )}
    </>
  );
}

/** What a linked repo is for, in one sentence (here and in the settings). */
export const REPOS_MEAN = "A linked GitHub repo brings its pull requests and issues into the project, and its pushes and events can start runs.";
/** What an environment is for, in one line (here and in the settings). */
export const ENVIRONMENTS_MEAN = "Where the project deploys: production, staging.";

/** The project's last three runs (UI audit #162), so its home covers each
 * of the bar's places: rows, as its rooms and issues are, each with its
 * state's bar while it waits or goes. None where runs can't be read.
 * None yet: Start a run (a cloud workspace first, where it has none) for
 * someone who can, else why not. */
function ProjectRuns({ p, workspaces }: { p: PlatformProject; workspaces: unknown[] | null }) {
  const [list, setList] = useState<RunView[] | null>(null);
  useEffect(() => {
    let live = true;
    runs.list(p.id, {}, 3).then(
      (r) => live && setList(r.runs),
      () => live && setList(null),
    );
    return () => void (live = false);
  }, [p.id]);
  if (list === null) return null;
  const all = pathOf({ page: "runs", project: p.id }, names);
  return (
    <>
      <SectionHead title="Runs" all={all} onAll={(e) => follow(e, all)} />
      {list.length === 0 &&
        (whyNoStart(p) ? (
          <EmptyState where="page" data-project-runs-why title="No runs yet." why={whyNoStart(p)} />
        ) : workspaces && !workspaces.length ? (
          <EmptyState
            where="page"
            data-project-runs-none
            title="No runs yet."
            why="A run is a command on one of the project's cloud workspaces. It has none yet: make one first."
            action={<NewWorkspaceButton project={p.id} />}
          />
        ) : (
          <EmptyState
            where="page"
            data-project-runs-none
            title="No runs yet."
            why="A run is a command on one of the project's cloud workspaces, by hand, on a schedule or on an event."
            action={
              <a class="primary" href={`${all}#new-run`} onClick={(e) => follow(e, `${all}#new-run`)} data-project-start-run>
                Start a run
              </a>
            }
          />
        ))}
      <ul class="platform-list card-list" data-project-runs>
        {list.map((r) => {
          const to = pathOf({ page: "run", project: p.id, run: r.id }, names);
          return (
            <Card as="li" dense key={r.id} tone={rowTone(runTone(r.state))} data-project-run={r.id}>
              <a href={to} onClick={(e) => follow(e, to)}>
                {runTitle(r.op, null, r.deploy)}
              </a>{" "}
              <span class="dim sw-mono">
                {STATES[r.state] ?? r.state} · <Time at={r.created_at} long />
              </span>
            </Card>
          );
        })}
      </ul>
    </>
  );
}

/** The team's members, read only, and the way to change them. Each with
 * their picture, round, as the bar's account button has it (UI audit
 * #162). */
function Members({ p }: { p: PlatformProject }) {
  const t = p.team!;
  return (
    <aside class="platform-members" data-project-members>
      <h2>People</h2>
      <ul class="chat-people">
        {t.members.map((m) => (
          <li key={m.account} data-member={m.account}>
            <MsgAvatar who={m.account} name={m.name} account={chat.on ? m.account : undefined} />
            <span>{m.name}</span> <span class="dim">{ROLE[m.role] ?? m.role}</span>
          </li>
        ))}
      </ul>
      {t.role === "owner" && (
        <p>
          <a href={controlOrigin()} data-manage-team>
            Manage team
          </a>
        </p>
      )}
    </aside>
  );
}

/** Nothing here (UI audit #170): a page's heading, why in a dim line, and
 * the way back to every project. */
export function Missing({ what }: { what: string }) {
  return (
    <section class="platform-section sw-page" data-platform-missing>
      <PageHeader title="Not found" sub={what} />
      <p>
        <a class="primary" href="/" onClick={(e) => follow(e, "/")}>
          Your projects
        </a>
      </p>
    </section>
  );
}
