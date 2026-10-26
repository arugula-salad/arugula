// `/p/:project`: a project's home (M150): its channels and direct
// messages, its hosted workspaces, its drafts waiting on linked repos
// (M170), and who's in its team. Membership shows read only: rosters
// change in the shell, signed by a device ("The platform page is on
// app.arugula.io, not on the keys' origin"), so the link goes there.

import { chat } from "../../shared/chat-store";
import { ProjectDrafts } from "../drafts";
import { MsgAvatar } from "../../shared/chat/avatar";
import { useSubscribe } from "../../shared/ui/hooks";
import { PageHeader } from "../../shared/ui/page-header";
import { names, roomPath } from "../links";
import { follow, pathOf } from "../router";
import { controlOrigin, session, type PlatformProject } from "../session";
import { roomTone, toneClass } from "../swarm";
import { HostedWorkspaceList, NoWorkspaces, useHostedWorkspaces } from "./hosted-workspace";
import { ProjectIssues } from "./issues";

const ROLE: Record<string, string> = { owner: "Owner", editor: "Editor", viewer: "Viewer" };

export function ProjectHome({ id }: { id: string }) {
  useSubscribe((fn) => session.subscribe(fn));
  useSubscribe((fn) => chat.subscribe(fn));
  const workspaces = useHostedWorkspaces(id);
  const p = session.project(id);
  if (!p) return <Missing what="That project isn't one of yours (or it's gone)." />;
  const rooms = chat.on ? chat.rooms.filter((r) => (p.team ? r.team === p.team.id : !r.team) && (r.kind === "channel" || r.kind === "dm")) : [];
  const chatPath = pathOf({ page: "chat", project: p.id }, names);
  const settingsPath = pathOf({ page: "project-settings", project: p.id }, names);
  const workspacesPath = pathOf({ page: "workspaces", project: p.id }, names);
  const nameOf = (account: string) =>
    account === session.me?.account ? "you" : (p.team?.members.find((m) => m.account === account)?.name ?? "someone who left");
  return (
    <section class="platform-section sw-page" data-project-home={p.id}>
      <PageHeader
        title={p.name}
        named
        sub={
          <>
            <a href={settingsPath} data-project-settings onClick={(e) => follow(e, settingsPath)}>
              Project settings
            </a>
            <span> · linked repos, environments</span>
          </>
        }
      />
      <div class="platform-columns">
        <div>
          <ProjectDrafts project={p.id} nameOf={nameOf} />
          <h2>
            <a href={chatPath} onClick={(e) => follow(e, chatPath)}>
              Chat
            </a>
          </h2>
          {chat.on === false && chat.signedOut && <p class="dim">You're signed out of Arugula control: reload the page to sign in again.</p>}
          {chat.on === false && !chat.signedOut && <p class="dim">Chat is off for you. A team owner turns it on in the team's Labs, on control's page.</p>}
          {chat.on && rooms.length === 0 && <p class="dim">No channels yet.</p>}
          <ul class="platform-list sw-rows" data-project-rooms>
            {rooms
              .sort((a, b) => Number(a.kind === "dm") - Number(b.kind === "dm") || a.name.localeCompare(b.name))
              .map((r) => {
                const to = roomPath(r.id);
                return (
                  <li key={r.id} class={`sw-row ${toneClass(roomTone(r.unread, r.mentions))}`}>
                    <a href={to} data-project-room={r.id} onClick={(e) => follow(e, to)}>
                      {r.kind === "channel" ? `# ${r.name}` : `@ ${r.with?.name ?? "someone who left"}`}
                      {r.unread > 0 && <span class={r.mentions ? "chat-count mention" : "chat-count"}>{r.unread}</span>}
                    </a>
                  </li>
                );
              })}
          </ul>
          {chat.on && <ProjectIssues p={p} />}
          {(session.hosted || !!workspaces?.length) && (
            <>
              <h2>
                <a href={workspacesPath} onClick={(e) => follow(e, workspacesPath)}>
                  Workspaces
                </a>
              </h2>
              {workspaces === null ? null : workspaces.length ? <HostedWorkspaceList project={p.id} list={workspaces} /> : <NoWorkspaces project={p.id} />}
            </>
          )}
        </div>
        {p.team && <Members p={p} />}
      </div>
    </section>
  );
}

/** The team's members, read only, and the way to change them. */
function Members({ p }: { p: PlatformProject }) {
  const t = p.team!;
  return (
    <aside class="platform-members" data-project-members>
      <h2>{t.name}</h2>
      <ul class="chat-people">
        {t.members.map((m) => (
          <li key={m.account} data-member={m.account}>
            <MsgAvatar who={m.account} name={m.name} />
            <span>{m.name}</span> <span class="dim">{ROLE[m.role] ?? m.role}</span>
          </li>
        ))}
      </ul>
      <p class="dim">
        {t.role === "owner" ? "Add people, change roles and take people out" : "Who's in it"} on{" "}
        <a href={controlOrigin()} data-manage-team>
          control's page
        </a>
        , under your account's Teams: a change to the team is signed by one of your devices there.
      </p>
    </aside>
  );
}

export function Missing({ what }: { what: string }) {
  return (
    <section class="platform-section" data-platform-missing>
      <p>{what}</p>
      <p>
        <a href="/" onClick={(e) => follow(e, "/")}>
          Your projects
        </a>
      </p>
    </section>
  );
}
