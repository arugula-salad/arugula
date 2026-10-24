// `/`: the reader's projects (M150), their own first, then each team's.
// Each is a card in the swarm's language (./swarm.css): its bar is the
// reader's role in it, its pill the people in its team.

import { useSubscribe } from "../../shared/ui/hooks";
import { names } from "../links";
import { follow, pathOf } from "../router";
import { session } from "../session";
import { roleTone, toneClass } from "../swarm";

export function Home() {
  useSubscribe((fn) => session.subscribe(fn));
  // A team's project is its default one, so this counts the teams too.
  const teams = session.projects.filter((p) => p.team).length;
  return (
    <section class="platform-section sw-page" data-platform-home>
      <h1>Projects</h1>
      <div class="sw-stats">
        <span>
          <b>{session.projects.length}</b> {session.projects.length === 1 ? "project" : "projects"}
        </span>
        <span>
          <b>{teams}</b> {teams === 1 ? "team" : "teams"}
        </span>
      </div>
      <ul class="platform-cards sw-cards">
        {session.projects.map((p) => {
          const to = pathOf({ page: "project", project: p.id }, names);
          const people = p.team?.members.length ?? 0;
          return (
            <li key={p.id}>
              <a class={`platform-card sw-card ${toneClass(roleTone(p.team?.role))}`} href={to} data-project={p.id} onClick={(e) => follow(e, to)}>
                <span class="sw-head">
                  <b class="sw-name">{p.name}</b>
                  {p.team && (
                    <span class="sw-n" title={`${people} ${people === 1 ? "person" : "people"} in ${p.team.name}`}>
                      {people}
                    </span>
                  )}
                </span>
                <span class="sw-id">{p.id.slice(0, 12)}</span>
                <span class="dim">
                  {p.team ? `${p.team.name}'s · ${people} ${people === 1 ? "person" : "people"} · you're ${p.team.role === "editor" ? "an editor" : `the ${p.team.role === "owner" ? "owner" : "viewer"}`}` : "Yours alone"}
                </span>
              </a>
            </li>
          );
        })}
      </ul>
    </section>
  );
}
