// `/`: the reader's projects (M150), their own first, then each team's.

import { useSubscribe } from "../../shared/ui/hooks";
import { names } from "../links";
import { follow, pathOf } from "../router";
import { session } from "../session";

export function Home() {
  useSubscribe((fn) => session.subscribe(fn));
  return (
    <section class="platform-section" data-platform-home>
      <h1>Projects</h1>
      <ul class="platform-cards">
        {session.projects.map((p) => {
          const to = pathOf({ page: "project", project: p.id }, names);
          return (
            <li key={p.id}>
              <a class="platform-card" href={to} data-project={p.id} onClick={(e) => follow(e, to)}>
                <b>{p.name}</b>
                <span class="dim">
                  {p.team ? `${p.team.name}'s · ${p.team.members.length} ${p.team.members.length === 1 ? "person" : "people"} · you're ${p.team.role === "editor" ? "an editor" : `the ${p.team.role === "owner" ? "owner" : "viewer"}`}` : "Yours alone"}
                </span>
              </a>
            </li>
          );
        })}
      </ul>
    </section>
  );
}
