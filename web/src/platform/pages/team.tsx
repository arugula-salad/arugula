// `/t/:team/chat`, `/settings` and `/members`: a team's own page (Arugula
// #895). People, chat and most settings are the team's, not one project's
// (aru-140), so they have a page of their own, reached from the bar's
// switcher. It says the team's name, and its way back is the project the
// reader was last in. Chat lists the team's channels; Members is its roster,
// which changes on the machines page, signed by a device.

import { chat } from "../../shared/chat-store";
import { EmptyLine } from "../../shared/ui/block-states";
import { useSubscribe } from "../../shared/ui/hooks";
import { PageHeader } from "../../shared/ui/page-header";
import { Chip } from "../../shared/ui/state-chip";
import { currentProject } from "../current-project";
import { names, roomPath, teamProject } from "../links";
import { ownerNamed, type Owner } from "../owners";
import { follow, go, pathOf, TEAM_TABS, type TeamTab } from "../router";
import { controlOrigin, session } from "../session";
import { Missing } from "./project";
import { OwnerSettings } from "./owner-settings";

const TAB_NAME: Record<TeamTab, string> = { chat: "Chat", settings: "Settings", members: "Members" };
const ROLE: Record<string, string> = { owner: "Owner", editor: "Editor", viewer: "Viewer" };

export function TeamPage({ team, tab }: { team: string; tab: TeamTab }) {
  useSubscribe((fn) => session.subscribe(fn));
  const owner = ownerNamed(session.projects, team);
  if (!owner || owner.kind !== "team") return <Missing what="That team isn't one of yours (or it's gone)." />;
  // Back to the project the reader was last in: team pages are in none.
  const last = session.project(currentProject());
  const back = last ? pathOf({ page: "project", project: last.id }, names) : "/";
  return (
    <section class="platform-section sw-page team-page" data-team-page={owner.key} data-team-tab={tab}>
      <PageHeader
        title={owner.name}
        named
        sub="Team"
        back={{ href: back, label: `Back to ${last?.name ?? "All projects"}`, onClick: (e) => follow(e, back), data: "team-back" }}
      />
      <div class="seg team-tabs" role="tablist" aria-label="Team">
        {TEAM_TABS.map((t) => (
          <button key={t} role="tab" aria-selected={t === tab} data-team-tab-button={t} onClick={() => go(pathOf({ page: "team", team: owner.key, tab: t }, names))}>
            {TAB_NAME[t]}
          </button>
        ))}
      </div>
      {tab === "chat" && <Channels owner={owner} />}
      {tab === "settings" && <OwnerSettings owner={owner} />}
      {tab === "members" && <Members owner={owner} />}
    </section>
  );
}

/** The team's channels, each opening in chat. */
function Channels({ owner }: { owner: Owner }) {
  useSubscribe((fn) => chat.subscribe(fn));
  if (chat.on === null) return <p class="dim">Loading…</p>;
  if (chat.on === false) return <EmptyLine data-team-chat-off>Chat is off.</EmptyLine>;
  const channels = chat.rooms.filter((r) => r.kind === "channel" && r.team === owner.key).sort((a, b) => a.name.localeCompare(b.name));
  const project = teamProject(owner.key);
  const all = project ? pathOf({ page: "chat", project }, names) : null;
  return (
    <div data-team-channels>
      {channels.length ? (
        <ul class="platform-list settings-rows">
          {channels.map((r) => {
            const to = roomPath(r.id);
            return (
              <li key={r.id} data-team-channel={r.id}>
                <a href={to} onClick={(e) => follow(e, to)}>
                  #{r.name}
                </a>
              </li>
            );
          })}
        </ul>
      ) : (
        <EmptyLine>No channels yet.</EmptyLine>
      )}
      {all && (
        <p>
          <a class="secondary" href={all} data-team-open-chat onClick={(e) => follow(e, all)}>
            Open chat
          </a>
        </p>
      )}
    </div>
  );
}

/** The team's roster, as the platform's copy has it. */
function Members({ owner }: { owner: Owner }) {
  return (
    <div data-team-members>
      <ul class="platform-list settings-rows">
        {owner.members.map((m) => (
          <li key={m.account} data-team-member={m.account}>
            <span>{m.name || m.account}</span>
            <Chip kind="tag">{ROLE[m.role] ?? m.role}</Chip>
          </li>
        ))}
      </ul>
      {!owner.members.length && <EmptyLine>None of its projects lists its people.</EmptyLine>}
      <p class="dim">
        Roles and invites change on <a href={controlOrigin()}>your machines page</a>, where each change is signed.
      </p>
    </div>
  );
}
