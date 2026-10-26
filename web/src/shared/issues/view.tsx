// One native issue (M201), wherever it's drawn (M203): the platform page's
// `/p/:project/issues/:issue`, and an issue block in a daemon workspace (a
// pin onto it). Its title and description, then its timeline (M194's
// events) and its thread (M196's object room, which is its comments) in
// one column, in time order: the room's own view (../chat/room), with the
// events between its messages. Agents' posts and events say whose agents
// they are (M197), and the room says when an agent grant reaches it.
// Owners and editors edit, close, reopen, label and assign; everyone in the
// project comments.
//
// The view reads and changes the issue through `IssueApiContext`: the
// platform's routes as the reader by default, or what the page says it.
// It reads it again when the platform says it changed (chat's live
// stream), when the page comes back into view, and when `refresh` moves.

import { createContext, type ComponentChildren, type ComponentType } from "preact";
import { useContext, useEffect, useRef, useState } from "preact/hooks";
import type { ChatRoom, IssueEvent, IssuePatch, IssuePatched, NativeIssue } from "../proto";
import { chat } from "../chat-store";
import { MsgAvatar } from "../chat/avatar";
import { RoomBody, type Between } from "../chat/room";
import { usePlatformApi } from "../platform-api";
import { Markdown } from "../ui/markdown";
import { usePhone, useSubscribe } from "../ui/hooks";
import { askText, openMenu, type MenuItem } from "../ui/menu";
import { IssueError, issues } from "./store";
import "./issues.css";
import { ago, eventLine, stateLabel, whoLabel, type NameOf } from "./text";

/** What an issue's view reads and changes it through. */
export interface IssueApi {
  get(id: string): Promise<NativeIssue>;
  events(id: string): Promise<IssueEvent[]>;
  change(id: string, p: Partial<IssuePatch>): Promise<IssuePatched>;
  /** Its thread (made the first time someone opens it). */
  thread(id: string): Promise<ChatRoom>;
  /** An issue already read. */
  known(id: string): NativeIssue | undefined;
  /** Every issue read, for the labels a project's issues have. */
  all(): NativeIssue[];
}

/** The platform's routes as the reader, unless the page says otherwise. */
export const IssueApiContext = createContext<IssueApi>(issues);

/** The issue's project as the reader is in it. */
export interface IssuePlace {
  /** Its id: an issue in another project isn't shown here. */
  project: string;
  /** Who's in it, for names, and whom to assign. */
  members: { account: string; name: string }[];
  /** Whether the reader opens and changes issues here. */
  writer: boolean;
  /** An account's name, when `members` doesn't have it. */
  nameOf?: NameOf;
}

/** What the view shows when there's no issue to show. */
function Gone({ what }: { what: string }) {
  return (
    <section class="issue-page" data-issue-missing>
      <p class="dim">{what}</p>
    </section>
  );
}

/** A label, as a chip. */
export function LabelChip({ label, onClick }: { label: string; onClick?: (e: MouseEvent) => void }) {
  return onClick ? (
    <button type="button" class="ws-tag issue-label" data-issue-label={label} onClick={onClick}>
      {label}
    </button>
  ) : (
    <span class="ws-tag issue-label" data-issue-label={label}>
      {label}
    </span>
  );
}

export function StateTag({ i }: { i: Pick<NativeIssue, "state" | "close_reason"> }) {
  return (
    <span class={`ws-tag forge-state ${i.state === "closed" ? "closed" : "open"}`} data-issue-state={i.state}>
      {stateLabel(i)}
    </span>
  );
}

const menuAt = (e: MouseEvent, items: MenuItem[]) => {
  const at = (e.currentTarget as HTMLElement).getBoundingClientRect();
  openMenu({ clientX: at.left, clientY: at.bottom + 4, preventDefault: () => e.preventDefault() }, items);
};

/** Whether a read failed because the platform couldn't be reached (or
 * didn't answer), not because it refused. */
const unreachable = (e: unknown) => !(e instanceof IssueError) || e.status >= 500;

export function IssueView({
  place,
  id,
  msg,
  back,
  foot,
  refresh,
  Missing = Gone,
  class: cls = "",
}: {
  place: IssuePlace;
  id: string;
  /** A comment to land on. */
  msg?: number;
  /** Over the title: the way back to a list. */
  back?: ComponentChildren;
  /** Under the thread: where it's read from. */
  foot?: ComponentChildren;
  /** Read it again when this changes (a block's word that it changed). */
  refresh?: unknown;
  Missing?: ComponentType<{ what: string }>;
  class?: string;
}) {
  const api = useContext(IssueApiContext);
  const { toast } = usePlatformApi();
  const phone = usePhone();
  const [issue, setIssue] = useState<NativeIssue | null>(api.known(id) ?? null);
  const [events, setEvents] = useState<IssueEvent[]>([]);
  const [room, setRoom] = useState<ChatRoom | null>(null);
  const [missing, setMissing] = useState<string | null>(null);
  const [stale, setStale] = useState<{ why: string; since: number } | null>(null);
  const [read, setRead] = useState<number | null>(null);
  const [threadError, setThreadError] = useState<string | null>(null);
  const [editing, setEditing] = useState<"title" | "body" | null>(null);
  const known = (account: string) => place.members.find((m) => m.account === account)?.name;
  const nameOf: NameOf = (account) => known(account) ?? place.nameOf?.(account);
  const writer = place.writer;

  // The issue and its timeline: now, again when the platform says it
  // changed, and when the page comes back into view (someone else may
  // have changed it meanwhile). Unreachable, what it said last stays.
  const again = () => {
    api.get(id).then(
      (i) => {
        setIssue(i);
        setMissing(null);
        setStale(null);
        setRead(Date.now());
      },
      (e: Error) => {
        if (unreachable(e)) setStale((had) => had ?? { why: e.message, since: Date.now() });
        else setMissing(e.message);
      },
    );
    api.events(id).then(setEvents, () => {});
  };
  useEffect(() => {
    again();
    api.thread(id).then(setRoom, (e: Error) => setThreadError(e.message));
    const off = chat.onObject(id, again);
    const back = () => document.visibilityState === "visible" && again();
    document.addEventListener("visibilitychange", back);
    return () => {
      off();
      document.removeEventListener("visibilitychange", back);
    };
  }, [id, api]);
  const said = useRef(refresh);
  useEffect(() => {
    if (said.current === refresh) return;
    said.current = refresh;
    again();
  }, [refresh]);
  // A comment came: the timeline may have moved too.
  useSubscribe((fn) => chat.subscribe(fn));

  if (missing && (!issue || !stale)) return <Missing what={`That issue isn't one you can read (${missing}).`} />;
  if (!issue) return stale ? <Missing what={`Arugula control can't be reached: ${stale.why}`} /> : <p class={`${cls} dim`}>Loading…</p>;
  if (issue.project !== place.project) return <Missing what="That issue is in another project." />;

  const change = async (patch: Partial<IssuePatch>) => {
    try {
      const r = await api.change(id, patch);
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

  const between: Between[] = events.flatMap((e) => {
    const line = eventLine(e, nameOf);
    return line === null ? [] : [{ key: `e${e.id}`, at: e.at, el: <IssueEventRow e={e} line={line} nameOf={nameOf} /> }];
  });

  return (
    <section class={`${cls} issue-page`} data-issue={issue.id} data-issue-number={issue.number} data-stale={stale ? "" : undefined}>
      {back && <p class="issue-back">{back}</p>}
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
        {stale && (
          <p class="issue-stale" role="status" data-issue-stale>
            Can't reach Arugula control now: this is what it said {read ? ago(read) : "last"}.
          </p>
        )}
      </header>
      <div class="issue-cols">
        <aside class="issue-side" aria-label="Labels and assignees">
          <Labels api={api} issue={issue} change={change} writer={writer} />
          <Assignees members={place.members} issue={issue} change={change} writer={writer} nameOf={nameOf} />
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
                  <Markdown text={issue.body} refs={null} />
                </div>
              ) : (
                <p class="dim">No description.</p>
              )}
            </div>
          </article>
          <div class="issue-thread chat-thread" data-issue-thread={room?.id}>
            {room ? (
              <RoomBody room={room} phone={phone} focus={msg} embedded name={`#${issue.number}`} between={between} empty="No comments yet. Everyone in the project reads them; write @name to notify someone." />
            ) : (
              <>
                {between.map((b) => (
                  <div key={b.key}>{b.el}</div>
                ))}
                <p class="thread-note">{threadError ? `The comments didn't load: ${threadError}` : "Loading the comments…"}</p>
              </>
            )}
          </div>
          {foot && <p class="issue-foot dim">{foot}</p>}
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

type Change = (x: Partial<IssuePatch>) => Promise<boolean>;

function Labels({ api, issue, change, writer }: { api: IssueApi; issue: NativeIssue; change: Change; writer: boolean }) {
  const pick = (e: MouseEvent) => {
    // The labels this page has seen in the project, and the issue's own.
    const seen = new Set(issue.labels);
    for (const i of api.all()) if (i.project === issue.project) for (const l of i.labels) seen.add(l);
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

function Assignees({
  members,
  issue,
  change,
  writer,
  nameOf,
}: {
  members: IssuePlace["members"];
  issue: NativeIssue;
  change: Change;
  writer: boolean;
  nameOf: NameOf;
}) {
  const pick = (e: MouseEvent) =>
    menuAt(e, [
      { header: "Assignees" },
      ...members.map((m) => {
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
