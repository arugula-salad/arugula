// `/p/:project`: a project's home (M150): its channels and direct
// messages, its Cloud workspaces, and who's in its team. Membership shows
// read only: rosters change in the shell, signed by a device ("The platform
// page is on app.arugula.io, not on the keys' origin"), so the link goes
// there.

import { chat } from "../../shared/chat-store";
import { cloudWorkspaces, cloudWorkspaceState } from "../../shared/cloud";
import { newCloudWorkspace } from "../cloud/workspaces";
import { MsgAvatar } from "../../shared/chat/avatar";
import { useSubscribe } from "../../shared/ui/hooks";
import { names, roomPath } from "../links";
import { follow, pathOf } from "../router";
import { controlOrigin, session, type PlatformProject } from "../session";

const ROLE: Record<string, string> = { owner: "Owner", editor: "Editor", viewer: "Viewer" };

export function ProjectHome({ id }: { id: string }) {
  useSubscribe((fn) => session.subscribe(fn));
  useSubscribe((fn) => chat.subscribe(fn));
  useSubscribe((fn) => cloudWorkspaces.subscribe(fn));
  const p = session.project(id);
  if (!p) return <Missing what="That project isn't one of yours (or it's gone)." />;
  const rooms = chat.on ? chat.rooms.filter((r) => (p.team ? r.team === p.team.id : !r.team) && (r.kind === "channel" || r.kind === "dm")) : [];
  const workspaces = p.team ? [] : cloudWorkspaces.list;
  const chatPath = pathOf({ page: "chat", project: p.id }, names);
  const settingsPath = pathOf({ page: "project-settings", project: p.id }, names);
  return (
    <section class="platform-section" data-project-home={p.id}>
      <h1>{p.name}</h1>
      <p>
        <a href={settingsPath} data-project-settings onClick={(e) => follow(e, settingsPath)}>
          Project settings
        </a>
        <span class="dim"> · linked repos</span>
      </p>
      <div class="platform-columns">
        <div>
          <h2>
            <a href={chatPath} onClick={(e) => follow(e, chatPath)}>
              Chat
            </a>
          </h2>
          {chat.on === false && <p class="dim">Chat is off for you. A team owner turns it on in the team's Labs, on control's page.</p>}
          {chat.on && rooms.length === 0 && <p class="dim">No channels yet.</p>}
          <ul class="platform-list" data-project-rooms>
            {rooms
              .sort((a, b) => Number(a.kind === "dm") - Number(b.kind === "dm") || a.name.localeCompare(b.name))
              .map((r) => {
                const to = roomPath(r.id);
                return (
                  <li key={r.id}>
                    <a href={to} data-project-room={r.id} onClick={(e) => follow(e, to)}>
                      {r.kind === "channel" ? `# ${r.name}` : `@ ${r.with?.name ?? "someone who left"}`}
                      {r.unread > 0 && <span class={r.mentions ? "chat-count mention" : "chat-count"}>{r.unread}</span>}
                    </a>
                  </li>
                );
              })}
          </ul>
          {!p.team && (
            <>
              <h2>
                <a href={pathOf({ page: "workspaces", project: p.id }, names)} onClick={(e) => follow(e, pathOf({ page: "workspaces", project: p.id }, names))}>
                  Cloud workspaces
                </a>{" "}
                <button class="chat-new" title="New Cloud workspace" aria-label="New Cloud workspace" data-project-new-workspace onClick={() => newCloudWorkspace()}>
                  +
                </button>
              </h2>
              {cloudWorkspaces.loaded && workspaces.length === 0 && <p class="dim">None yet.</p>}
              <ul class="platform-list" data-project-workspaces>
                {workspaces.map((w) => {
                  const to = pathOf({ page: "workspace", project: p.id, workspace: w.id }, names);
                  return (
                    <li key={w.id}>
                      <a href={to} data-project-workspace={w.id} onClick={(e) => follow(e, to)}>
                        {w.name} <span class="dim">· {cloudWorkspaceState(w).word}</span>
                      </a>
                    </li>
                  );
                })}
              </ul>
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
