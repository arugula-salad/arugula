// `/`: the reader's projects (M150), their own first, then each team's.
// Each is a card in the swarm's language (./swarm.css): its bar is the
// reader's role in it, its pill the people in its team.

import { Card } from "../../shared/ui/card";
import { useSubscribe } from "../../shared/ui/hooks";
import { PageHeader } from "../../shared/ui/page-header";
import { names } from "../links";
import { follow, pathOf } from "../router";
import { session } from "../session";
import { roleTone } from "../swarm";
import { Count } from "../../shared/ui/state-chip";

export function Home() {
  useSubscribe((fn) => session.subscribe(fn));
  return (
    <section class="platform-section sw-page" data-platform-home>
      <PageHeader
        title="Projects"
        stats={
          <div class="sw-stats">
            <span>
              <b>{session.projects.length}</b> {session.projects.length === 1 ? "project" : "projects"}
            </span>
          </div>
        }
      />
      <ul class="card-grid">
        {session.projects.map((p) => {
          const to = pathOf({ page: "project", project: p.id }, names);
          const people = p.team?.members.length ?? 0;
          return (
            <li key={p.id}>
              <Card as="a" tone={roleTone(p.team?.role)} href={to} data-project={p.id} onClick={(e) => follow(e, to)}>
                <span class="card-head">
                  <b class="card-title big">{p.name}</b>
                  {p.team && (
                    <Count class="count-end" title={`${people} ${people === 1 ? "person" : "people"} in ${p.team.name}`}>
                      {people}
                    </Count>
                  )}
                </span>
                <span class="card-id" title={p.id}>
                  {p.id.slice(0, 12)}
                </span>
                <span class="dim" title={p.team ? `Team ${p.team.name} · you're ${p.team.role === "editor" ? "an editor" : `the ${p.team.role === "owner" ? "owner" : "viewer"}`}` : undefined}>
                  {p.team ? `Team · ${people} ${people === 1 ? "person" : "people"}` : "Yours alone"}
                </span>
              </Card>
            </li>
          );
        })}
      </ul>
    </section>
  );
}
