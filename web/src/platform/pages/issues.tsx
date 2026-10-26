// `/p/:project/issues` and `/p/:project/issues/new` (M201): a project's
// native issues (M194, through M195's routes), open or closed, by label,
// assignee or words, and a new one. Owners and editors open issues;
// viewers read them and comment. Where the project's new issues go (its
// issue home, M195) is set at the foot of the list, and a project with
// issues both here and on a linked repo says so. Issues come with chat's
// Labs flag, as their threads do: while it's off there are none.

import { useEffect, useRef, useState } from "preact/hooks";
import type { IssueHome, NativeIssue } from "../../shared/proto";
import { chat } from "../../shared/chat-store";
import { MsgAvatar } from "../../shared/chat/avatar";
import { useSubscribe } from "../../shared/ui/hooks";
import { openMenu, type MenuItem } from "../../shared/ui/menu";
import { PageHeader } from "../../shared/ui/page-header";
import { issues, type IssueFilter } from "../../shared/issues/store";
import { ago, githubNewIssue, labelsOf, stateLabel, whoLabel } from "../../shared/issues/text";
import { LabelChip } from "../../shared/issues/view";
import { names } from "../links";
import { follow, go, pathOf } from "../router";
import { session, type PlatformProject } from "../session";
import { toast } from "../toast";
import { issueTone, toneClass } from "../swarm";
import { ThreadUnread } from "../thread-unread";
import { SectionHead } from "../section-head";
import { Missing } from "./project";

/** Who's in a project: its team's roster, or its one person. */
export function membersOf(p: PlatformProject): { account: string; name: string }[] {
  if (p.team) return p.team.members.map((m) => ({ account: m.account, name: m.name }));
  const me = session.me;
  return me ? [{ account: me.account, name: me.login || me.name }] : [];
}

/** An account's name in a project, for its issues' pages. */
export function nameIn(p: PlatformProject) {
  return (account: string) => membersOf(p).find((m) => m.account === account)?.name;
}

/** Whether the reader opens and changes issues here: a personal project's
 * person, a team's owners and editors. */
export function mayWrite(p: PlatformProject): boolean {
  return !p.team || p.team.role !== "viewer";
}

/** The project, once chat says issues are on; else what to show. */
export function useIssuesOn(id: string): { p: PlatformProject } | { show: preact.JSX.Element } {
  useSubscribe((fn) => session.subscribe(fn));
  useSubscribe((fn) => chat.subscribe(fn));
  const p = session.project(id);
  if (!p) return { show: <Missing what="That project isn't one of yours (or it's gone)." /> };
  if (chat.on === null) return { show: <p class="platform-section dim">Loading…</p> };
  if (!chat.on) return { show: <IssuesOff /> };
  return { p };
}

export function IssuesOff() {
  return (
    <section class="platform-section sw-page" data-issues-off>
      <PageHeader title="Issues" />
      <p class="dim">Issues come with chat, which is off for you. A team owner turns it on in the team's Labs, on Control.</p>
    </section>
  );
}

/** What the list holds, as the runs' list counts its runs: the issues
 * shown (a page of them), and of them those assigned to the reader. */
function IssueStats({ list, me }: { list: NativeIssue[]; me?: string }) {
  const mine = me ? list.filter((i) => i.assignees.includes(me)).length : 0;
  return (
    <div class="sw-stats" data-issues-stats>
      <span>
        <b>{list.length}</b> shown
      </span>
      {mine > 0 && (
        <span class="needs">
          <b>{mine}</b> assigned to you
        </span>
      )}
    </div>
  );
}

const menuAt = (e: MouseEvent, items: MenuItem[]) => {
  const at = (e.currentTarget as HTMLElement).getBoundingClientRect();
  openMenu({ clientX: at.left, clientY: at.bottom + 4, preventDefault: () => e.preventDefault() }, items);
};

export function IssuesPage({ project, filter }: { project: string; filter: IssueFilter }) {
  const on = useIssuesOn(project);
  if ("show" in on) return on.show;
  return <IssueList p={on.p} filter={filter} />;
}

function IssueList({ p, filter }: { p: PlatformProject; filter: IssueFilter }) {
  const [list, setList] = useState<NativeIssue[] | null>(null);
  const [more, setMore] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [home, setHome] = useHome(p.id);
  const [typed, setTyped] = useState(filter.q ?? "");
  const nameOf = nameIn(p);
  const key = JSON.stringify([p.id, filter.state, filter.label, filter.assignee, filter.q]);
  useEffect(() => {
    let live = true;
    setError(null);
    issues.list(p.id, filter).then(
      (r) => live && (setList(r.issues), setMore(r.more)),
      (e: Error) => live && setError(e.message),
    );
    return () => void (live = false);
  }, [key]);

  // Typing searches as it goes; the address keeps the words.
  const typing = useRef<number | undefined>(undefined);
  const to = (f: IssueFilter, replace = false) => go(pathOf({ page: "issues", project: p.id, ...f }, names), replace);
  const search = (q: string) => {
    setTyped(q);
    clearTimeout(typing.current);
    typing.current = window.setTimeout(() => to({ ...filter, q: q.trim() || undefined }, true), 250);
  };
  // Every narrowing off, as the runs' list's Clear: open or closed stays.
  const clear = () => {
    clearTimeout(typing.current);
    setTyped("");
    to({ state: filter.state });
  };
  const next = async () => {
    const last = list?.at(-1);
    if (!last) return;
    try {
      const r = await issues.list(p.id, filter, last.number);
      setList([...(list ?? []), ...r.issues]);
      setMore(r.more);
    } catch (e) {
      setError((e as Error).message);
    }
  };

  const seen = [...new Set([...(list ?? []).flatMap((i) => i.labels), ...(filter.label ? [filter.label] : [])])].sort();
  const labelMenu = (e: MouseEvent) =>
    menuAt(e, [
      { header: "Label" },
      { label: "Any label", checked: !filter.label, run: () => to({ ...filter, label: undefined }) },
      ...seen.map((l) => ({ label: l, checked: filter.label === l, run: () => to({ ...filter, label: l }) })),
    ]);
  const assigneeMenu = (e: MouseEvent) =>
    menuAt(e, [
      { header: "Assignee" },
      { label: "Anyone", checked: !filter.assignee, run: () => to({ ...filter, assignee: undefined }) },
      ...membersOf(p).map((m) => ({ label: m.name, checked: filter.assignee === m.account, run: () => to({ ...filter, assignee: m.account }) })),
    ]);
  const exportMenu = (e: MouseEvent) =>
    menuAt(e, [
      { header: "Every issue, its timeline and comments" },
      { label: "Export as JSON", run: () => location.assign(issues.exportUrl(p.id, "json")) },
      { label: "Export as Markdown", run: () => location.assign(issues.exportUrl(p.id, "md")) },
    ]);
  const narrowed = !!(filter.q || filter.label || filter.assignee);
  const onGithub = home?.home === "github" && home.repos.length > 0;

  return (
    <section class="platform-section sw-page issues-page" data-issues={p.id}>
      <PageHeader
        title="Issues"
        actions={
          <>
            <button class="issues-export" title="Download every issue" data-issues-export onClick={exportMenu}>
              Export <span class="caret">▾</span>
            </button>
            {mayWrite(p) && <NewIssueButton p={p} home={home} />}
          </>
        }
        stats={!!list?.length && <IssueStats list={list} me={session.me?.account} />}
      />
      {home?.both && (
        <p class="issues-note" role="note" data-issues-both>
          This project has issues in two places: {home.native_issues} here, and more on {home.repos.join(", ")} on GitHub.{" "}
          {home.home === "github" ? "New ones go to GitHub." : "New ones are kept here."}
        </p>
      )}
      <div class="issues-tools">
        <div class="issues-states" role="group" aria-label="Open or closed">
          <button aria-pressed={filter.state !== "closed"} data-issues-open onClick={() => to({ ...filter, state: undefined })}>
            Open
          </button>
          <button aria-pressed={filter.state === "closed"} data-issues-closed onClick={() => to({ ...filter, state: "closed" })}>
            Closed
          </button>
        </div>
        <input type="search" class="issues-search" placeholder="Search issues" aria-label="Search issues" value={typed} data-issues-search onInput={(e) => search(e.currentTarget.value)} />
        <button class={filter.label ? "issues-filter on" : "issues-filter"} data-issues-label onClick={labelMenu}>
          {filter.label ? `Label: ${filter.label}` : "Label"} <span class="caret">▾</span>
        </button>
        <button class={filter.assignee ? "issues-filter on" : "issues-filter"} data-issues-assignee onClick={assigneeMenu}>
          {filter.assignee ? `Assignee: ${nameOf(filter.assignee) ?? "someone"}` : "Assignee"} <span class="caret">▾</span>
        </button>
        {narrowed && (
          <button class="link" data-issues-clear onClick={clear}>
            Clear
          </button>
        )}
      </div>
      {error && <p class="control-error" role="alert">{error}</p>}
      {list === null && !error && <p class="dim">Loading…</p>}
      {list?.length === 0 && (
        <p class="dim issues-none" data-issues-none>
          {narrowed
            ? `No ${filter.state === "closed" ? "closed" : "open"} issues match.`
            : filter.state === "closed"
              ? "No closed issues."
              : onGithub
                ? `No open issues here. This project's issues are on ${home!.repos.join(", ")} on GitHub.`
                : mayWrite(p)
                  ? "No open issues. Open one with New issue."
                  : "No open issues."}
        </p>
      )}
      <ul class="issues-list sw-cards" data-issues-list>
        {list?.map((i) => (
          <IssueRow key={i.id} p={p} i={i} nameOf={nameOf} label={(l) => to({ ...filter, label: l })} />
        ))}
      </ul>
      {more && (
        <button class="chat-older" data-issues-more onClick={() => void next()}>
          More issues
        </button>
      )}
      {mayWrite(p) && home && <IssueHomeSetting p={p} home={home} set={setHome} />}
    </section>
  );
}

/** Where the project's new issues go, once the platform has said: null
 * until then, so nothing offers the wrong place meanwhile. */
function useHome(project: string): [IssueHome | null, (h: IssueHome) => void] {
  const [home, setHome] = useState<IssueHome | null>(null);
  useEffect(() => {
    let live = true;
    setHome(null);
    issues.home(project).then((h) => live && setHome(h), () => {});
    return () => void (live = false);
  }, [project]);
  return [home, setHome];
}

/** "New issue": the form here, or GitHub's page when the project's issues
 * live there (a menu of its repos when it links several). Nothing until
 * the home is known. `small`: the project home's, beside its heading. */
function NewIssueButton({ p, home, small }: { p: PlatformProject; home: IssueHome | null; small?: boolean }) {
  if (!home) return null;
  // The project home's is a small button beside its heading (UI audit #162).
  const cls = small ? "secondary small" : "primary issues-new";
  const label = home.home === "github" ? `${small ? "+ " : ""}New issue on GitHub ↗` : small ? "+ New issue" : "New issue";
  const title = home.home === "github" ? "New issue, on GitHub" : "New issue";
  if (home.home === "github" && home.repos.length === 1) {
    return (
      <a class={cls} href={githubNewIssue(home.repos[0])} target="_blank" rel="noopener" title={title} aria-label={title} data-issue-new-github>
        {label}
      </a>
    );
  }
  if (home.home === "github" && home.repos.length > 1) {
    return (
      <button class={cls} title={title} aria-label={title} data-issue-new-github onClick={(e) => menuAt(e, [{ header: "New issue on" }, ...home.repos.map((r) => ({ label: `${r} ↗`, run: () => void open(githubNewIssue(r), "_blank", "noopener") }))])}>
        {label}
        <span class="caret"> ▾</span>
      </button>
    );
  }
  const to = pathOf({ page: "new-issue", project: p.id }, names);
  return (
    <a class={cls} href={to} title={title} data-issue-new={small ? undefined : ""} data-project-new-issue={small ? "" : undefined} onClick={(e) => follow(e, to)}>
      {label}
    </a>
  );
}

function IssueRow({ p, i, nameOf, label }: { p: PlatformProject; i: NativeIssue; nameOf: (a: string) => string | undefined; label: (l: string) => void }) {
  const to = pathOf({ page: "issue", project: p.id, issue: i.id }, names);
  return (
    // A card in the swarm's language (../swarm.css), as a run's is: its bar
    // is the issue's state (open ok, closed muted: done, not failed).
    <li class={`issue-row sw-card ${toneClass(issueTone(i.state))}`} data-issue-row={i.number} data-issue-row-state={i.state}>
      <div class="issue-row-main">
        <a href={to} class="issue-row-title" onClick={(e) => follow(e, to)}>
          {i.title}
        </a>
        <ThreadUnread target={i.id} />
        {i.labels.map((l) => (
          <LabelChip key={l} label={l} onClick={() => label(l)} />
        ))}
        <span class="issue-row-meta dim">
          {stateLabel(i)} · #{i.number} {i.state === "closed" && i.closed_at ? `closed ${ago(i.closed_at)}` : `opened ${ago(i.created_at)} by ${whoLabel(i.author, i.agent, nameOf)}`}
        </span>
      </div>
      {i.assignees.length > 0 && (
        <span class="issue-row-who" title={`Assigned to ${i.assignees.map((a) => nameOf(a) ?? "someone who left").join(", ")}`}>
          {i.assignees.slice(0, 3).map((a) => (
            <MsgAvatar key={a} who={a} name={nameOf(a) ?? "?"} />
          ))}
        </span>
      )}
    </li>
  );
}

/** Where the project's new issues go (M195): kept here, or on its linked
 * repo on GitHub. Owners and editors choose; it starts on GitHub while the
 * project has a linked repo. */
function IssueHomeSetting({ p, home, set }: { p: PlatformProject; home: IssueHome; set: (h: IssueHome) => void }) {
  const choose = async (h: string | null) => {
    try {
      set(await issues.setHome(p.id, h));
    } catch (e) {
      toast(`Couldn't change it: ${(e as Error).message}`);
    }
  };
  return (
    <div class="issues-home" data-issue-home={home.home}>
      <h2>Where new issues go</h2>
      {home.repos.length === 0 ? (
        <p class="dim">Kept here: this project has no linked GitHub repo, so its issues are its own.</p>
      ) : (
        <>
          <label class="control-confirm">
            <input type="radio" name="issue-home" checked={home.home === "native"} data-issue-home-native onChange={() => void choose("native")} /> Here, on Arugula
          </label>
          <label class="control-confirm">
            <input type="radio" name="issue-home" checked={home.home === "github"} data-issue-home-github onChange={() => void choose("github")} /> On GitHub ({home.repos.join(", ")})
          </label>
          <p class="dim">
            {home.chosen ? (
              <>
                Chosen for this project.{" "}
                <button class="link" data-issue-home-reset onClick={() => void choose(null)}>
                  Back to the default
                </button>
              </>
            ) : (
              "The default for a project with a linked repo."
            )}
          </p>
        </>
      )}
    </div>
  );
}

// ---- a new issue

export function NewIssuePage({ project }: { project: string }) {
  const on = useIssuesOn(project);
  if ("show" in on) return on.show;
  return <NewIssue p={on.p} />;
}

function NewIssue({ p }: { p: PlatformProject }) {
  const [title, setTitle] = useState("");
  const [body, setBody] = useState("");
  const [labels, setLabels] = useState("");
  const [assignees, setAssignees] = useState<string[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [home] = useHome(p.id);
  const list = pathOf({ page: "issues", project: p.id }, names);
  const back = { href: list, label: "Issues", onClick: (e: MouseEvent) => follow(e, list) };
  if (!mayWrite(p)) {
    return (
      <section class="platform-section sw-page" data-issue-new-viewer>
        <PageHeader title="New issue" back={back} />
        <p class="dim">You're a viewer in {p.team?.name}: you read its issues and comment on them, and its owners and editors open them.</p>
      </section>
    );
  }
  if (!home) return <p class="platform-section dim">Loading…</p>;
  if (home.home === "github") {
    return (
      <section class="platform-section sw-page issue-new" data-issue-new-github-only>
        <PageHeader title="New issue" back={back} />
        <p>
          This project's new issues go to GitHub
          {home.repos.length > 1 ? ": open one on the repo it's about." : "."}
        </p>
        <ul class="platform-list">
          {home.repos.map((r) => (
            <li key={r}>
              <a href={githubNewIssue(r)} target="_blank" rel="noopener" data-issue-new-github={r}>
                New issue on {r} ↗
              </a>
            </li>
          ))}
        </ul>
        <p class="dim">An owner or editor can keep them here instead, under "Where new issues go" on the issues list.</p>
      </section>
    );
  }
  const people = membersOf(p);
  const submit = async (e: Event) => {
    e.preventDefault();
    if (!title.trim() || busy) return;
    setBusy(true);
    setError(null);
    try {
      const i = await issues.create(p.id, { title: title.trim(), body, labels: labelsOf(labels), assignees });
      go(pathOf({ page: "issue", project: p.id, issue: i.id }, names), true);
    } catch (err) {
      setError((err as Error).message);
      setBusy(false);
    }
  };
  return (
    <section class="platform-section sw-page issue-new" data-issue-new-form>
      <PageHeader title="New issue" back={back} />
      <form class="issue-form" onSubmit={(e) => void submit(e)}>
        <label>
          Title
          <input required maxLength={256} value={title} autoFocus data-issue-title-input onInput={(e) => setTitle(e.currentTarget.value)} />
        </label>
        <label>
          Description
          <textarea rows={8} value={body} placeholder="What's it about?" data-issue-body-input onInput={(e) => setBody(e.currentTarget.value)} />
        </label>
        <label>
          <span>
            Labels <span class="dim">(comma-separated)</span>
          </span>
          <input value={labels} placeholder="bug, web" data-issue-labels-input onInput={(e) => setLabels(e.currentTarget.value)} />
        </label>
        {people.length > 0 && (
          <fieldset class="issue-assignees">
            <legend>Assignees</legend>
            {people.map((m) => (
              <label key={m.account} class="control-confirm">
                <input
                  type="checkbox"
                  checked={assignees.includes(m.account)}
                  data-issue-assignee-input={m.account}
                  onChange={(e) => setAssignees(e.currentTarget.checked ? [...assignees, m.account] : assignees.filter((a) => a !== m.account))}
                />{" "}
                {m.name}
              </label>
            ))}
          </fieldset>
        )}
        {error && (
          <p role="alert" class="control-error">
            {error}
          </p>
        )}
        <div class="prompt-buttons">
          <a href={list} class="issue-cancel" onClick={(e) => follow(e, list)}>
            Cancel
          </a>
          <button class="primary" disabled={busy || !title.trim()} data-issue-create>
            {busy ? "Opening…" : "Open issue"}
          </button>
        </div>
      </form>
    </section>
  );
}

// ---- on the project's home

/** The project's newest open issues, and the way to the rest (and to a new
 * one, for those who open them). */
export function ProjectIssues({ p }: { p: PlatformProject }) {
  const [list, setList] = useState<NativeIssue[] | null>(null);
  useEffect(() => {
    let live = true;
    issues.list(p.id, {}, undefined, 5).then(
      (r) => live && setList(r.issues),
      () => live && setList([]),
    );
    return () => void (live = false);
  }, [p.id]);
  const [home] = useHome(p.id);
  const all = pathOf({ page: "issues", project: p.id }, names);
  return (
    <>
      <SectionHead title="Issues" all={all} data="project-issues-link" action={mayWrite(p) && <NewIssueButton p={p} home={home} small />} />
      {list?.length === 0 && <p class="dim">No open issues.</p>}
      {/* Rows, as the home's rooms are (UI audit #162): a title and its
          number, no underline until hovered. */}
      <ul class="platform-list sw-rows" data-project-issues>
        {list?.map((i) => {
          const to = pathOf({ page: "issue", project: p.id, issue: i.id }, names);
          return (
            <li key={i.id} class="sw-row">
              <a href={to} data-project-issue={i.number} onClick={(e) => follow(e, to)}>
                {i.title}
              </a>{" "}
              <span class="dim sw-mono">#{i.number}</span>
            </li>
          );
        })}
      </ul>
    </>
  );
}
