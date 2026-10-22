// `/p/:project/issues/:issue` (M201): one native issue. Its title and
// description, then its timeline (M194's events) and its thread (M196's
// object room, which is its comments) in one column, in time order: the
// room's own view (../../shared/chat/room), with the events between its
// messages. Agents' posts and events say whose agents they are (M197), and
// the room says when an agent grant reaches it. Owners and editors edit,
// close, reopen, label and assign; everyone in the project comments.

import { useEffect, useState } from "preact/hooks";
import type { ChatRoom, IssueEvent, NativeIssue } from "../../shared/proto";
import { chat } from "../../shared/chat-store";
import { MsgAvatar } from "../../shared/chat/avatar";
import { RoomBody, type Between } from "../../shared/chat/room";
import { Markup } from "../../shared/ui/markup";
import { usePhone, useSubscribe } from "../../shared/ui/hooks";
import { askText, openMenu, type MenuItem } from "../../shared/ui/menu";
import { issues } from "../issues/store";
import { ago, eventLine, whoLabel, type NameOf } from "../issues/text";
import { names } from "../links";
import { follow, pathOf } from "../router";
import { type PlatformProject } from "../session";
import { toast } from "../toast";
import { LabelChip, StateTag, mayWrite, membersOf, nameIn, useIssuesOn } from "./issues";
import { Missing } from "./project";

export function IssuePage({ project, issue, msg }: { project: string; issue: string; msg?: number }) {
  const on = useIssuesOn(project);
  if ("show" in on) return on.show;
  return <IssueView key={issue} p={on.p} id={issue} msg={msg} />;
}

const menuAt = (e: MouseEvent, items: MenuItem[]) => {
  const at = (e.currentTarget as HTMLElement).getBoundingClientRect();
  openMenu({ clientX: at.left, clientY: at.bottom + 4, preventDefault: () => e.preventDefault() }, items);
};

function IssueView({ p, id, msg }: { p: PlatformProject; id: string; msg?: number }) {
  const phone = usePhone();
  const [issue, setIssue] = useState<NativeIssue | null>(issues.known(id) ?? null);
  const [events, setEvents] = useState<IssueEvent[]>([]);
  const [room, setRoom] = useState<ChatRoom | null>(null);
  const [missing, setMissing] = useState<string | null>(null);
  const [threadError, setThreadError] = useState<string | null>(null);
  const [editing, setEditing] = useState<"title" | "body" | null>(null);
  const nameOf = nameIn(p);
  const writer = mayWrite(p);

  // The issue and its timeline: now, and again when the page comes back
  // into view (someone else may have changed it meanwhile).
  const read = () => {
    issues.get(id).then(setIssue, (e: Error) => setMissing(e.message));
    issues.events(id).then(setEvents, () => {});
  };
  useEffect(() => {
    read();
    issues.thread(id).then(setRoom, (e: Error) => setThreadError(e.message));
    const back = () => document.visibilityState === "visible" && read();
    document.addEventListener("visibilitychange", back);
    return () => document.removeEventListener("visibilitychange", back);
  }, [id]);
  // A comment came: the timeline may have moved too.
  useSubscribe((fn) => chat.subscribe(fn));

  if (missing && !issue) return <Missing what={`That issue isn't one you can read (${missing}).`} />;
  if (!issue) return <p class="platform-section dim">Loading…</p>;
  if (issue.project !== p.id) return <Missing what="That issue is in another project." />;

  const change = async (patch: Parameters<typeof issues.change>[1]) => {
    try {
      const r = await issues.change(id, patch);
      setIssue(r.issue);
      setEvents((had) => [...had, ...r.events.filter((e) => !had.some((x) => x.id === e.id))]);
      return true;
    } catch (e) {
      toast(`Couldn't change it: ${(e as Error).message}`);
      return false;
    }
  };

  const closeMenu = (e: MouseEvent) =>
    menuAt(e, [
      { header: "Close as" },
      ...(["completed", "not_planned", "duplicate"] as const).map((reason) => ({
        label: reason === "completed" ? "Completed: it's done" : reason === "not_planned" ? "Not planned: won't be done" : "A duplicate of another issue",
        run: () => void change({ state: "closed", reason }),
      })),
    ]);

  const list = pathOf({ page: "issues", project: p.id }, names);
  const between: Between[] = events.flatMap((e) => {
    const line = eventLine(e, nameOf);
    return line === null ? [] : [{ key: `e${e.id}`, at: e.at, el: <IssueEventRow e={e} line={line} nameOf={nameOf} /> }];
  });

  return (
    <section class="platform-section issue-page" data-issue={issue.id} data-issue-number={issue.number}>
      <p class="issue-back">
        <a href={list} onClick={(e) => follow(e, list)} data-issue-back>
          ‹ Issues
        </a>
      </p>
      <header class="issue-head">
        {editing === "title" ? (
          <TitleEdit
            title={issue.title}
            done={async (t) => {
              if (t === null || t === issue.title || (await change({ title: t }))) setEditing(null);
            }}
          />
        ) : (
          <h1>
            <span data-issue-title>{issue.title}</span> <span class="dim">#{issue.number}</span>
          </h1>
        )}
        <div class="issue-meta">
          <StateTag i={issue} />
          <span class="dim">
            <b class="issue-who">{whoLabel(issue.author, issue.agent, nameOf)}</b> opened this {ago(issue.created_at)}
            {issue.state === "closed" && issue.closed_at ? ` · closed ${ago(issue.closed_at)}` : ""}
          </span>
          {issue.agent && (
            <span class="msg-badge agent-badge" title={`Opened by an agent on ${issue.agent.machine_name || "a machine"}, through its owner's grant`}>
              agent
            </span>
          )}
          <span class="issue-actions">
            {writer && editing !== "title" && (
              <button class="issue-action" data-issue-edit-title onClick={() => setEditing("title")}>
                Edit title
              </button>
            )}
            {writer &&
              (issue.state === "closed" ? (
                <button class="issue-action" data-issue-reopen onClick={() => void change({ state: "open" })}>
                  Reopen
                </button>
              ) : (
                <span class="issue-split">
                  <button class="issue-action" data-issue-close onClick={() => void change({ state: "closed", reason: "completed" })}>
                    Close issue
                  </button>
                  <button class="issue-action" title="Close it for another reason" aria-label="Close it for another reason" data-issue-close-as onClick={closeMenu}>
                    ▾
                  </button>
                </span>
              ))}
          </span>
        </div>
      </header>
      <div class="issue-cols">
        <aside class="issue-side" aria-label="Labels and assignees">
          <Labels p={p} issue={issue} change={change} writer={writer} />
          <Assignees p={p} issue={issue} change={change} writer={writer} nameOf={nameOf} />
        </aside>
        <div class="issue-main">
          <article class="issue-post" data-issue-body>
            <div class="msg-gutter">
              {issue.agent ? <MsgAvatar who={`agent:${issue.agent.machine}`} name={issue.agent.machine_name || "agent"} /> : <MsgAvatar who={issue.author || "gone"} name={nameOf(issue.author) ?? "?"} />}
            </div>
            <div class="issue-post-body">
              <div class="thread-head">
                <span class="thread-name">{whoLabel(issue.author, issue.agent, nameOf)}</span>
                <time title={new Date(issue.created_at).toLocaleString()}>{ago(issue.created_at)}</time>
                {writer && editing !== "body" && (
                  <button class="issue-action small" data-issue-edit-body onClick={() => setEditing("body")}>
                    Edit
                  </button>
                )}
              </div>
              {editing === "body" ? (
                <BodyEdit
                  body={issue.body}
                  done={async (b) => {
                    if (b === null || b === issue.body || (await change({ body: b }))) setEditing(null);
                  }}
                />
              ) : issue.body.trim() ? (
                <div class="issue-text">
                  <Markup text={issue.body} />
                </div>
              ) : (
                <p class="dim">No description.</p>
              )}
            </div>
          </article>
          <div class="issue-thread chat-thread" data-issue-thread={room?.id}>
            {room ? (
              <RoomBody room={room} phone={phone} focus={msg} name={`#${issue.number}`} between={between} empty="No comments yet. Everyone in the project reads them; write @name to notify someone." />
            ) : (
              <>
                {between.map((b) => (
                  <div key={b.key}>{b.el}</div>
                ))}
                <p class="thread-note">{threadError ? `The comments didn't load: ${threadError}` : "Loading the comments…"}</p>
              </>
            )}
          </div>
        </div>
      </div>
    </section>
  );
}

function IssueEventRow({ e, line, nameOf }: { e: IssueEvent; line: string; nameOf: NameOf }) {
  return (
    <div class={`issue-event ${e.kind}`} data-issue-event={e.kind} data-agent={e.agent?.machine}>
      <span class="issue-event-mark" aria-hidden="true">
        {e.kind === "closed" ? "✓" : e.kind === "reopened" ? "↺" : e.kind.includes("label") ? "◆" : e.kind.includes("assign") ? "●" : "✎"}
      </span>
      <span>
        <b>{whoLabel(e.actor, e.agent, nameOf)}</b> {line}{" "}
        <time class="dim" title={new Date(e.at).toLocaleString()}>
          {ago(e.at)}
        </time>
      </span>
    </div>
  );
}

function Labels({ p, issue, change, writer }: { p: PlatformProject; issue: NativeIssue; change: (x: Parameters<typeof issues.change>[1]) => Promise<boolean>; writer: boolean }) {
  const pick = (e: MouseEvent) => {
    // The labels this page has seen in the project, and the issue's own.
    const seen = new Set(issue.labels);
    for (const l of knownLabels(p.id)) seen.add(l);
    const items: MenuItem[] = [
      { header: "Labels" },
      ...[...seen].sort().map((l) => {
        const on = issue.labels.includes(l);
        return { label: l, checked: on, run: () => void change({ labels: on ? { remove: [l] } : { add: [l] } }) };
      }),
      "separator",
      {
        label: "New label…",
        run: async () => {
          const v = (await askText("Add a label", "", "one or a few words"))?.trim();
          if (v) void change({ labels: { add: [v] } });
        },
      },
    ];
    menuAt(e, items);
  };
  return (
    <section class="issue-side-part" data-issue-labels>
      <h3>
        Labels
        {writer && (
          <button class="issue-action small" title="Add or take off labels" data-issue-labels-edit onClick={pick}>
            Edit
          </button>
        )}
      </h3>
      {issue.labels.length ? (
        <div class="ws-tags">
          {issue.labels.map((l) => (
            <LabelChip key={l} label={l} />
          ))}
        </div>
      ) : (
        <p class="dim">None</p>
      )}
    </section>
  );
}

/** Labels on the project's issues the page has read. */
function knownLabels(project: string): string[] {
  const out = new Set<string>();
  for (const i of issues.all()) if (i.project === project) for (const l of i.labels) out.add(l);
  return [...out];
}

function Assignees({
  p,
  issue,
  change,
  writer,
  nameOf,
}: {
  p: PlatformProject;
  issue: NativeIssue;
  change: (x: Parameters<typeof issues.change>[1]) => Promise<boolean>;
  writer: boolean;
  nameOf: NameOf;
}) {
  const pick = (e: MouseEvent) =>
    menuAt(e, [
      { header: "Assignees" },
      ...membersOf(p).map((m) => {
        const on = issue.assignees.includes(m.account);
        return { label: m.name, checked: on, run: () => void change({ assignees: on ? { remove: [m.account] } : { add: [m.account] } }) };
      }),
    ]);
  return (
    <section class="issue-side-part" data-issue-assignees>
      <h3>
        Assignees
        {writer && (
          <button class="issue-action small" title="Assign people in the project" data-issue-assignees-edit onClick={pick}>
            Edit
          </button>
        )}
      </h3>
      {issue.assignees.length ? (
        <ul class="chat-people">
          {issue.assignees.map((a) => (
            <li key={a} data-issue-assignee={a}>
              <MsgAvatar who={a} name={nameOf(a) ?? "?"} />
              <span>{nameOf(a) ?? "someone who left"}</span>
            </li>
          ))}
        </ul>
      ) : (
        <p class="dim">Nobody</p>
      )}
    </section>
  );
}

function TitleEdit({ title, done }: { title: string; done: (t: string | null) => void }) {
  const [t, setT] = useState(title);
  return (
    <form
      class="issue-title-edit"
      onSubmit={(e) => {
        e.preventDefault();
        if (t.trim()) done(t.trim());
      }}
    >
      <input value={t} maxLength={256} autoFocus aria-label="Title" data-issue-title-edit onInput={(e) => setT(e.currentTarget.value)} onKeyDown={(e) => e.key === "Escape" && done(null)} />
      <button class="primary" disabled={!t.trim()}>
        Save
      </button>
      <button type="button" onClick={() => done(null)}>
        Cancel
      </button>
    </form>
  );
}

function BodyEdit({ body, done }: { body: string; done: (b: string | null) => void }) {
  const [b, setB] = useState(body);
  return (
    <form
      class="issue-form issue-body-edit"
      onSubmit={(e) => {
        e.preventDefault();
        done(b);
      }}
    >
      <textarea rows={8} value={b} autoFocus aria-label="Description" data-issue-body-edit onInput={(e) => setB(e.currentTarget.value)} onKeyDown={(e) => e.key === "Escape" && done(null)} />
      <div class="prompt-buttons">
        <button type="button" onClick={() => done(null)}>
          Cancel
        </button>
        <button class="primary">Save</button>
      </div>
    </form>
  );
}

