// `/p/:project/sessions` (Fleet 5): a project's agent sessions in one
// list, a wave of them, beside its runs. Each row is a session's state,
// its issue, its box, branch and HEAD, when it last pushed, who started it
// (its asker, or their agent), and what waits on the reader: its question
// or a permission, CI that failed again, its PR to send.
//
// It narrows by its issue's label, by a tracker (an issue's number: the
// issues its body names as #N, and those whose body names it), by who
// asked, and to the ones going. A row reads again when its session's live
// event comes (`chat.onObject`), and the list now and then while any goes.

import { Card } from "../../shared/ui/card";
import { useEffect, useState } from "preact/hooks";
import { chat } from "../../shared/chat-store";
import { stateOf } from "../../shared/chat/session-card";
import type { SessionRow, SessionWave } from "../../shared/proto.gen";
import { platformFetch, refusalOf } from "../../shared/maintenance";
import { BlockError, EmptyLine, EmptyState } from "../../shared/ui/block-states";
import { useSubscribe } from "../../shared/ui/hooks";
import { openMenu, type MenuItem } from "../../shared/ui/menu";
import { PageHeader } from "../../shared/ui/page-header";
import { StateChip } from "../../shared/ui/state-chip";
import { Time } from "../../shared/ui/time";
import "../../shared/issues/issues.css";
import { names } from "../links";
import { waveQuery, type WaveFilter } from "../paths";
import { follow, go, pathOf } from "../router";
import { session, type PlatformProject } from "../session";
import { ListLoading } from "./issues";
import { Missing } from "./project";

async function wave(project: string, f: WaveFilter): Promise<SessionWave> {
  const qs = waveQuery(f);
  const res = await platformFetch(
    `/api/projects/${encodeURIComponent(project)}/sessions${qs ? `?${qs}` : ""}`,
    { credentials: "same-origin" },
  );
  const j = (await res.json().catch(() => ({}))) as SessionWave & {
    error?: string;
  };
  if (!res.ok) throw new Error(refusalOf(res.status, j, `HTTP ${res.status}`));
  return j;
}

const menuAt = (e: MouseEvent, items: MenuItem[]) => {
  const at = (e.currentTarget as HTMLElement).getBoundingClientRect();
  openMenu(
    {
      clientX: at.left,
      clientY: at.bottom + 4,
      preventDefault: () => e.preventDefault(),
    },
    items,
  );
};

export function SessionsPage({
  project,
  filter,
}: {
  project: string;
  filter: WaveFilter;
}) {
  useSubscribe((fn) => session.subscribe(fn));
  const p = session.project(project);
  if (!p)
    return <Missing what="That project isn't one of yours (or it's gone)." />;
  return <Wave p={p} filter={filter} />;
}

function Wave({ p, filter }: { p: PlatformProject; filter: WaveFilter }) {
  const [w, setW] = useState<SessionWave | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [tick, setTick] = useState(0);
  const key = JSON.stringify([p.id, filter]);
  useEffect(() => {
    let live = true;
    setError(null);
    wave(p.id, filter).then(
      (r) => live && setW(r),
      (e: Error) => live && setError(e.message),
    );
    return () => void (live = false);
  }, [key, tick]);
  // Each session's live event reads the wave again; so does a new one,
  // now and then, while any goes.
  const ids = (w?.sessions ?? []).map((s) => s.session).join(" ");
  useEffect(() => {
    const again = () => setTick((n) => n + 1);
    const offs = ids
      ? ids.split(" ").map((id) => chat.onObject(id, again))
      : [];
    return () => offs.forEach((off) => off());
  }, [ids]);
  const going = !!w?.sessions.some((s) => !s.settled);
  useEffect(() => {
    if (!going) return;
    const t = setInterval(
      () => document.visibilityState === "visible" && setTick((n) => n + 1),
      15_000,
    );
    return () => clearInterval(t);
  }, [going]);

  const to = (f: WaveFilter) =>
    go(pathOf({ page: "sessions", project: p.id, ...f }, names));
  const labelMenu = (e: MouseEvent) =>
    menuAt(e, [
      { header: "Its issue's label" },
      {
        label: "Any label",
        checked: !filter.label,
        run: () => to({ ...filter, label: undefined }),
      },
      ...(w?.labels ?? []).map((l) => ({
        label: l,
        checked: filter.label === l,
        run: () => to({ ...filter, label: l }),
      })),
    ]);
  const askerMenu = (e: MouseEvent) =>
    menuAt(e, [
      { header: "Asked by" },
      {
        label: "Anyone",
        checked: !filter.asker,
        run: () => to({ ...filter, asker: undefined }),
      },
      {
        label: "Me",
        checked: filter.asker === "me",
        run: () => to({ ...filter, asker: "me" }),
      },
      ...(w?.askers ?? []).map(([a, name]) => ({
        label: name || a,
        checked: filter.asker === a,
        run: () => to({ ...filter, asker: a }),
      })),
    ]);
  const narrowed = !!(
    filter.label ||
    filter.tracker !== undefined ||
    filter.asker ||
    filter.going
  );
  const askerName =
    filter.asker === "me"
      ? "me"
      : (w?.askers.find(([a]) => a === filter.asker)?.[1] ?? "one");
  const waiting = w?.sessions.filter((s) => s.waiting).length ?? 0;

  return (
    <section
      class="platform-section sw-page sessions-page"
      data-sessions={p.id}
    >
      <PageHeader
        title="Agent sessions"
        sub="The project's agent sessions, newest first: where each runs, its branch and last push, and what waits on you. Narrow it to a track by its issues' label, or by a tracker issue."
        stats={
          !!w?.sessions.length && (
            <span class="dim" data-sessions-stats>
              {w.sessions.filter((s) => !s.settled).length} going · {waiting}{" "}
              waiting on you
            </span>
          )
        }
      />
      <div class="issues-tools">
        <button
          class="secondary"
          aria-pressed={!!filter.label}
          data-sessions-label
          onClick={labelMenu}
        >
          {filter.label ? `Label: ${filter.label}` : "Label"}{" "}
          <span class="caret">▾</span>
        </button>
        <TrackerFilter filter={filter} to={to} tracker={w?.tracker ?? null} />
        <button
          class="secondary"
          aria-pressed={!!filter.asker}
          data-sessions-asker
          onClick={askerMenu}
        >
          {filter.asker ? `Asked by ${askerName}` : "Asked by"}{" "}
          <span class="caret">▾</span>
        </button>
        <label class="issues-filter">
          <input
            type="checkbox"
            checked={!!filter.going}
            data-sessions-going
            onChange={(e) =>
              to({
                ...filter,
                going:
                  (e.currentTarget as HTMLInputElement).checked || undefined,
              })
            }
          />{" "}
          Going only
        </label>
        {narrowed && (
          <button class="link" data-sessions-clear onClick={() => to({})}>
            Clear
          </button>
        )}
      </div>
      {w === null && error ? (
        <BlockError inline title="Can't read this project's sessions." detail={error} retry={() => setTick((n) => n + 1)} />
      ) : (
        error && (
          <p class="control-error" role="alert">
            {error}
          </p>
        )
      )}
      {w === null && !error && <ListLoading />}
      {w?.sessions.length === 0 &&
        (narrowed ? (
          <EmptyLine data-sessions-none>No sessions match.</EmptyLine>
        ) : (
          <EmptyState
            where="page"
            data-sessions-none
            title="No agent sessions yet."
            why="Agent on this on an issue, or @arugula in a channel, starts one."
          />
        ))}
      <ul class="issues-list card-list" data-sessions-list>
        {w?.sessions.map((s) => (
          <Row key={s.session} p={p} s={s} />
        ))}
      </ul>
      {w?.more && <p class="dim">More match than are listed: narrow it.</p>}
    </section>
  );
}

/** The tracker it's narrowed to, or a field for an issue's number. */
function TrackerFilter({
  filter,
  to,
  tracker,
}: {
  filter: WaveFilter;
  to: (f: WaveFilter) => void;
  tracker: SessionWave["tracker"];
}) {
  const [text, setText] = useState("");
  if (filter.tracker !== undefined)
    return (
      <button
        class="secondary"
        aria-pressed="true"
        data-sessions-tracker={filter.tracker}
        title="Show every session again"
        onClick={() => to({ ...filter, tracker: undefined })}
      >
        Tracker #{filter.tracker}
        {tracker ? ` ${tracker.title}` : ""} ✕
      </button>
    );
  return (
    <form
      class="issues-filter"
      onSubmit={(e) => {
        e.preventDefault();
        const n = Number(text.replace(/^#/, ""));
        if (Number.isInteger(n) && n > 0) to({ ...filter, tracker: n });
      }}
    >
      <input
        size={9}
        placeholder="Tracker #N"
        aria-label="A tracker issue's number"
        value={text}
        data-sessions-tracker-input
        onInput={(e) => setText((e.currentTarget as HTMLInputElement).value)}
      />
    </form>
  );
}

function Row({ p, s }: { p: PlatformProject; s: SessionRow }) {
  const path = pathOf(
    { page: "session", project: p.id, session: s.session },
    names,
  );
  const issue = s.issue
    ? pathOf({ page: "issue", project: p.id, issue: s.issue.id }, names)
    : null;
  const st = stateOf(s.state, false);
  return (
    <Card
      as="li"
      class="issue-row"
      tone={st.tone}
      data-session-row={s.session}
      data-session-state={s.state}
    >
      <div class="issue-row-main">
        <a href={path} class="issue-row-title" onClick={(e) => follow(e, path)}>
          {s.issue ? `#${s.issue.number} ${s.issue.title}` : s.branch}
        </a>
        {s.waiting && (
          <StateChip tone="warn" data-session-waiting>
            Waits on you: {s.waiting}
          </StateChip>
        )}
        <span class="issue-row-meta dim">
          <StateChip tone={st.tone}>{st.label}</StateChip> · on{" "}
          {s.workspace_name || s.workspace} · <code>{s.branch}</code>
          {s.head ? (
            <>
              {" "}
              at <code>{s.head}</code>
            </>
          ) : null}
          {" · "}
          {s.pushed_at ? (
            <>
              pushed <Time at={s.pushed_at} />
            </>
          ) : (
            "not pushed"
          )}
          {" · "}
          {s.started_by ?? `asked by ${s.asker_name || "someone"}`}
          {s.pr && (
            <>
              {" · "}
              <a href={s.pr} target="_blank" rel="noopener">
                {s.merged_at ? "PR merged" : "PR"}
              </a>
            </>
          )}
          {issue && (
            <>
              {" · "}
              <a href={issue} onClick={(e) => follow(e, issue)}>
                its issue
              </a>
            </>
          )}
        </span>
        {s.question && (
          <span class="issue-row-meta">It asks: {s.question}</span>
        )}
      </div>
      <code class="card-id" title={s.session}>
        {s.session.replace(/^ses_/, "").slice(0, 8)}
      </code>
    </Card>
  );
}
