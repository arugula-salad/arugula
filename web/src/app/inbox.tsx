// The Inbox (Lift 6d, #1486; v6 §2.6, shot 7): everything for you across
// your projects, grouped by project, as the server lists it (`GET
// /api/inbox`, One Inbox #1433). The project you're in comes first, then
// the rest as the rail has them; one with nothing says so on its line.
// What's in no project (a DM) comes last. In each: what you decide (asks,
// permissions, approvals; then to-dos), then what names you (a review
// asked, a mention), then chat, then alerts (checks failed on your PR). A
// teammate's asks fold into one line ("2 asks for Jake ›").
//
// Every row is the Inbox's (./needs): it expands and answers in place, and
// `↵` goes to its place, in whatever project, through the back stack, so
// `esc` comes back here. What isn't an ask can be dismissed. The rail's
// Inbox badge is the server's `count`; each project's line is its
// `by_project`, the same as its mark on the rail. Drafts waiting for you to
// send (M170), which the server doesn't list, follow as they were.
//
// Beside it, the agent panel (#1528), as the Overview has it: moving
// through the rows peeks each one's asker there (its pane's session, or the
// chief of staff of its project), so an ask answers in either place, and
// answered in one it's gone from both (the row reads the server's list; the
// panel, the machine's pane or the room's card).

import { useState } from "preact/hooks";
import type { InboxItem } from "../shared/proto";
import { Avatar } from "../shared/kit/avatar";
import { Button } from "../shared/kit/button";
import { NeedList } from "../shared/kit/need-row";
import { Row } from "../shared/kit/row";
import { usePanel } from "../shared/kit/panel";
import { CHIEF_KEY, panelKey, type PanelTarget } from "../shared/kit/panel-key";
import { Surface } from "../shared/kit/surface";
import { usePhone, useSubscribe } from "../shared/ui/hooks";
import type { Fleet } from "../shell/fleet";
import { currentProject, onCurrentProject } from "../platform/current-project";
import { DraftList, useMyDrafts } from "../platform/drafts";
import { asksFor, byTeammate, forTeammate, memberName, useInbox } from "../platform/inbox";
import { owners } from "../platform/owners";
import { session } from "../platform/session";
import { toast } from "../platform/toast";
import { ALERTS, openItem, paneOfItem, paneTarget, useNeeds } from "./needs";
import "./inbox.css";

/** Where an item goes in its project: decisions, to-dos, what names you, chat, alerts. */
function rank(i: InboxItem): number {
  if (ALERTS.has(i.kind)) return 4;
  if (i.group === "decide") return i.kind === "todo" ? 1 : 0;
  return i.group === "named" ? 2 : 3;
}

/** The projects' order: the one you're in, then the rail's. */
function projectOrder(): { id: string; name: string }[] {
  const all = owners(session.projects).flatMap((o) => o.projects);
  const here = currentProject();
  return [...all.filter((p) => p.id === here), ...all.filter((p) => p.id !== here)];
}

export function InboxPlace({ fleet }: { fleet: Fleet | null }) {
  useSubscribe((fn) => session.subscribe(fn));
  useSubscribe(onCurrentProject);
  useSubscribe((fn) => fleet?.subscribe(fn) ?? (() => {}));
  const { inbox, error } = useInbox();
  const drafts = useMyDrafts();
  const me = session.me?.account;
  const phone = usePhone();
  const AgentPanel = usePanel();
  // What the panel shows: the project of the row the keys are on (else the
  // one you're in), its asker's tab, and the tab in front (none: the chief of staff).
  const [peeked, setPeeked] = useState<{ project?: string; target?: PanelTarget; current?: string }>({});
  const peek = (i: InboxItem) => {
    const pane = fleet && paneOfItem(fleet, i);
    const target = fleet && pane ? paneTarget(fleet, pane, "agent") : undefined;
    setPeeked((was) => {
      // Another project's asker: its own panel; this one's: a tab beside the last.
      const project = i.subject.project ?? was.project;
      const kept = project === was.project ? was.target : undefined;
      return target ? { project, target, current: panelKey(target) } : { project, target: kept, current: undefined };
    });
  };
  const needs = useNeeds({ me, fleet, toast, peek, goes: true });
  const [unfolded, setUnfolded] = useState<ReadonlySet<string>>(new Set());

  const live = (inbox?.items ?? []).filter((i) => i.group !== "ready" && !i.done_at);
  const all = needs.kept(live).sort((a, b) => rank(a) - rank(b));
  const projects = projectOrder();
  const known = new Set(projects.map((p) => p.id));
  // A project the list names that the reader's list doesn't (yet): by its id.
  const strays = [...new Set(all.flatMap((i) => (i.subject.project && !known.has(i.subject.project) ? [i.subject.project] : [])))].map((id) => ({ id, name: id }));
  const groups = [...projects, ...strays].map((p) => ({ ...p, items: all.filter((i) => i.subject.project === p.id) }));
  const loose = all.filter((i) => !i.subject.project);
  if (loose.length) groups.push({ id: "", name: "Not in a project", items: loose });

  // The rows the keys move through, as drawn: each project's own, then its unfolded teammates'.
  const shown = groups.map((g) => {
    const mine = g.items.filter((i) => !forTeammate(i, me));
    const theirs = byTeammate(g.items, me);
    return { ...g, mine, theirs };
  });
  needs.order(shown.flatMap((g) => [...g.mine, ...g.theirs.flatMap(([who, list]) => (unfolded.has(`${g.id} ${who}`) ? list : []))]));

  // What isn't an ask is dismissed here; an ask is answered.
  const extra = (i: InboxItem, answers: boolean) => (
    <>
      {(!answers || i.subject.type === "pane") && !!i.subject.url && (
        <Button keys="↵" onClick={() => openItem(i)}>
          Open
        </Button>
      )}
      {i.group !== "decide" && (
        <Button data-inbox-done={i.id} disabled={needs.busy === i.id} onClick={() => void needs.dismiss(i)}>
          Dismiss
        </Button>
      )}
    </>
  );

  const by = inbox?.by_project ?? {};
  const mineDrafts = drafts.drafts ?? [];
  const here = currentProject();
  const panelProject = peeked.project ?? (here && known.has(here) ? here : projects[0]?.id);
  const panel = AgentPanel && panelProject && (
    <div class="inbox-side" data-inbox-side>
      <AgentPanel
        key={panelProject}
        project={panelProject}
        targets={peeked.target ? [peeked.target] : []}
        current={peeked.current}
        onPick={(k) => setPeeked((was) => ({ ...was, current: k === CHIEF_KEY ? undefined : k }))}
        onClose={() => setPeeked((was) => ({ project: was.project }))}
      />
    </div>
  );
  return (
    <div class={phone ? "inbox-page phone" : "inbox-page"}>
      <section class="inbox" aria-label="Inbox" data-platform-inbox data-inbox-count={inbox?.count ?? 0}>
        {error && <p class="control-error">{error}</p>}
        {inbox === null && !error && <p class="dim">Loading…</p>}
        {inbox !== null && all.length === 0 && (
          <p class="inbox-quiet" data-inbox-empty>
            Nothing wants you right now.
          </p>
        )}
        {inbox !== null && (
          <Surface kind="section" as="section" class="inbox-list">
            {shown.map((g) => {
              const n = g.id ? (by[g.id] ?? 0) : 0;
              return (
                <div key={g.id} class="inbox-project" data-inbox-project={g.id || "none"}>
                  <header class="inbox-head">
                    <Avatar name={g.name} />
                    <h2>{g.name}</h2>
                    {g.items.length === 0 ? (
                      <span class="m-meta">nothing for you</span>
                    ) : (
                      <span class="m-meta" data-inbox-project-count={n}>
                        {n || ""}
                      </span>
                    )}
                  </header>
                  {g.mine.length > 0 && <NeedList label={g.name}>{g.mine.map((i) => needs.row(i, extra))}</NeedList>}
                  {g.theirs.map(([who, list]) => {
                    const key = `${g.id} ${who}`;
                    return (
                      <div key={key} data-inbox-for={who}>
                        <Row label="" title={asksFor(list.length, memberName(who))} open={unfolded.has(key)} onClick={() => setUnfolded((s) => (s.has(key) ? new Set([...s].filter((x) => x !== key)) : new Set([...s, key])))}>
                          <NeedList label={`For ${memberName(who)}`}>{list.map((i) => needs.row(i, extra))}</NeedList>
                        </Row>
                      </div>
                    );
                  })}
                </div>
              );
            })}
          </Surface>
        )}
        {/* Only when there are some: a read that fails says nothing here. */}
        {mineDrafts.length > 0 && (
          <Surface kind="section" as="section" class="inbox-list" data-inbox-group="draft">
            <header class="inbox-head">
              <h2>Drafts waiting for you</h2>
              <span class="m-meta">{mineDrafts.length}</span>
            </header>
            <DraftList drafts={mineDrafts} nameOf={() => "you"} onChange={() => void drafts.reload()} />
          </Surface>
        )}
      </section>
      {panel}
    </div>
  );
}
