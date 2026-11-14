// `/p/:project/settings/move`: "Move to project…" (Arugula #971), for the
// project's owner, from its settings. Its target is a new project (a name and
// an owner) or one that exists; on the left, what moves (repos first, then
// boards, Docs and loose issues); on the right, what to know before moving,
// then what comes along, as counts that open. Before you move shows only what
// the person has to deal with, and nothing when there's nothing. No options:
// each case does one thing, as the platform's planner says (./plan), asked
// again a moment after each pick.
//
// A machine's Docs on a moving board are its library's, which only it moves
// (`doc move`): where the page reaches that machine (the app, on the
// reader's own machine that answers) and the target is a team's, they can be
// picked, and the machine moves them once the rest has; anywhere else the
// group says where they move from, with a link there.

import { useEffect, useRef, useState } from "preact/hooks";
import type { ProjectDoc } from "../../shared/proto.gen";
import { usePlatformApi } from "../../shared/platform-api";
import { projectDocs } from "../../shared/docs/store";
import { Card } from "../../shared/ui/card";
import { useSubscribe } from "../../shared/ui/hooks";
import { ArrowIcon } from "../../shared/ui/icons";
import { PageHeader } from "../../shared/ui/page-header";
import { CountList, PickList, type Pick, type PickGroup } from "../../shared/ui/pick-list";
import { Section } from "../../shared/ui/settings-section";
import { names } from "../links";
import { makers, ownerKey, owners, PERSONAL } from "../owners";
import { Missing } from "../pages/project";
import { follow, go, pathOf } from "../router";
import { session, type PlatformProject } from "../session";
import { toast } from "../toast";
import { askOf, changed, matches, move, noPicks, plan, type Picks, type Plan } from "./plan";
import "./move.css";

/** A list longer than this gets a search (as a menu of 13 rows gets a filter). */
const SEARCH_FROM = 13;

/** How long picking waits before asking the planner again. */
const SETTLE_MS = 250;

/** The plan's lists that come a page at a time. */
type Paged = "repos" | "boards" | "issues" | "docs";

export function MovePage({ project }: { project: string }) {
  useSubscribe((fn) => session.subscribe(fn));
  const p = session.project(project);
  if (!p) return <Missing what="That project isn't one of yours (or it's gone)." />;
  const all = owners(session.projects);
  if (all.find((o) => o.key === ownerKey(p))?.role !== "owner") return <Missing what="Only the project's owner moves work out of it." />;
  return <Mover key={p.id} p={p} />;
}

function Mover({ p }: { p: PlatformProject }) {
  const api = usePlatformApi();
  const [, tick] = useState(0);
  // A machine's Docs are pickable while the page reaches it.
  useEffect(() => api.machineDocs?.watch(() => tick((n) => n + 1)), []);
  const all = owners(session.projects);
  const mine = ownerKey(p);
  // Who a new project can be made for; existing projects the reader may write.
  const makes = makers(all);
  const writes = new Set(all.filter((o) => o.role === "owner" || o.role === "editor").map((o) => o.key));
  const targets = session.projects.filter((x) => x.id !== p.id && !x.archived && writes.has(ownerKey(x)));
  const [picks, setPicks] = useState<Picks>(() => noPicks(mine));
  const [q, setQ] = useState({ repos: "", boards: "", docs: "", issues: "" });
  const [out, setOut] = useState<Plan | null>(null);
  const [err, setErr] = useState("");
  const [said, setSaid] = useState("");
  const [busy, setBusy] = useState(false);
  // The project's Docs, by title: the list alone, not each one read.
  const [docs, setDocs] = useState<ProjectDoc[] | null>(null);
  useEffect(() => {
    let live = true;
    projectDocs.list(p.id).then(
      (l) => live && setDocs(l.docs),
      () => live && setDocs([]),
    );
    return () => void (live = false);
  }, [p.id]);

  // Ask the planner again once the picks settle; an older answer is dropped.
  const ask = askOf(picks);
  const key = JSON.stringify(ask);
  const asked = useRef(key);
  asked.current = key;
  const first = useRef(true);
  useEffect(() => {
    const stop = new AbortController();
    const t = setTimeout(
      () =>
        plan(p.id, ask, stop.signal).then(
          (v) => {
            setOut(v);
            setErr("");
          },
          (e: Error) => !stop.signal.aborted && setErr(e.message),
        ),
      first.current ? 0 : SETTLE_MS,
    );
    first.current = false;
    return () => {
      clearTimeout(t);
      stop.abort();
    };
  }, [key]);

  /** The next page of one of the plan's lists, onto what's shown. */
  const more = (list: Paged) => {
    const next = out?.along[list].next;
    if (next === undefined) return;
    const was = key;
    void plan(p.id, { ...ask, after: { [list]: next } }).then((v) =>
      setOut((had) => {
        // Picks changed since: that plan's pages aren't this one's.
        if (!had || asked.current !== was) return had;
        const page = v.along[list];
        return { ...had, along: { ...had.along, [list]: { ...page, items: [...had.along[list].items, ...page.items] } } };
      }),
    );
  };

  const set = (f: (n: Picks) => void) =>
    setPicks((old) => {
      const n = { ...old, repos: new Set(old.repos), boards: new Set(old.boards), issues: new Set(old.issues), docs: new Set(old.docs), machineDocs: new Set(old.machineDocs) };
      f(n);
      return n;
    });
  const flip = (which: Set<string>, k: string, on: boolean) => void (on ? which.add(k) : which.delete(k));

  const isNew = picks.target === "new";
  const target = isNew ? null : targets.find((t) => t.id === picks.target);
  // A machine gives its Docs only to a team's project.
  const toTeam = isNew ? picks.owner !== PERSONAL : !!target && ownerKey(target) !== PERSONAL;
  const here = (machine: string) => toTeam && !!api.machineDocs?.here(machine);
  const to = isNew ? picks.name.trim() || "the new project" : (target?.name ?? out?.to.name ?? "");
  const src = out?.source;
  const moving = new Set(out?.along.boards.items.map((b) => b.id));

  const moveIt = async () => {
    setBusy(true);
    setErr("");
    setSaid("");
    try {
      const done = await move(p.id, askOf(picks));
      // Then each machine its Docs, into the project the rest went to.
      const stayed: string[] = [];
      const listed = new Set(out?.along.machine_docs.flatMap((m) => m.docs.map((d) => `${m.machine}/${d.id}`)));
      for (const k of picks.machineDocs) {
        if (!listed.has(k)) continue;
        const [machine, doc] = k.split("/");
        const why = await api.machineDocs!.move(machine, doc, done.to.id);
        if (why) stayed.push(why);
      }
      await session.load();
      toast(stayed.length ? `Moved to ${done.to.name}. ${stayed.length === 1 ? "A Doc" : `${stayed.length} Docs`} stayed on its machine: ${stayed[0]}` : `Moved to ${done.to.name}.`);
      go(pathOf({ page: "project", project: done.to.id }, names));
    } catch (e) {
      if (!changed(e)) {
        setErr((e as Error).message);
        return;
      }
      // Something moved under the plan: nothing did here. The plan as it is now.
      try {
        setOut(await plan(p.id, askOf(picks)));
        setSaid("Something changed; here's the new plan.");
      } catch (again) {
        setErr((again as Error).message);
      }
    } finally {
      setBusy(false);
    }
  };

  const settings = pathOf({ page: "project-settings", project: p.id }, names);
  const repoRows = (src?.repos ?? []).filter((r) => matches(r.name, q.repos));
  const boardRows = (src?.boards ?? []).filter((b) => matches(b.title, q.boards));
  const along = out?.along;
  return (
    <section class="platform-section sw-page move-page" data-move-page={p.id}>
      <PageHeader title="Move to project" back={{ href: settings, label: `${p.name} settings`, onClick: (e) => follow(e, settings) }} sub={`From ${p.name}.`} />
      <div class="move-cols">
        <div class="move-col" data-move-left>
          <Section id="move-to" title="To">
            <div class="move-fields">
              <label>
                Project
                <select class="field" value={picks.target} data-move-target onChange={(e) => set((n) => void (n.target = e.currentTarget.value))}>
                  <option value="new">A new project</option>
                  {targets.map((t) => (
                    <option key={t.id} value={t.id}>
                      {t.name} ({all.find((o) => o.key === ownerKey(t))?.name ?? "Personal"})
                    </option>
                  ))}
                </select>
              </label>
              {isNew && (
                <>
                  <label>
                    Name
                    <input class="field" value={picks.name} placeholder="Factorio" maxLength={100} data-move-name onInput={(e) => set((n) => void (n.name = e.currentTarget.value))} />
                  </label>
                  <label>
                    Owner
                    <select class="field" value={picks.owner} data-move-owner onChange={(e) => set((n) => void (n.owner = e.currentTarget.value))}>
                      {makes.map((o) => (
                        <option key={o.key} value={o.key}>
                          {o.name}
                        </option>
                      ))}
                    </select>
                  </label>
                </>
              )}
            </div>
          </Section>
          <Section id="move-what" title="What moves" lede="A repo brings its boards, issues and sessions.">
            {!src ? (
              <p class="dim" data-move-loading>
                Reading {p.name}…
              </p>
            ) : (
              <>
                <h3 class="move-sub">Repos</h3>
                <PickList
                  label="Repos"
                  data-move-repos
                  groups={[{ key: "repos", items: repoRows.map((r): Pick => ({ key: r.name, title: r.name, detail: repoDetail(r), on: picks.repos.has(r.name) })) }]}
                  toggle={(k, on) => set((n) => flip(n.repos, k, on))}
                  search={searchOver(src.repos.length, q.repos, (v) => setQ({ ...q, repos: v }), "Search repos", repoRows.map((r) => r.name), picks.repos, (keys, on) => set((n) => keys.forEach((k) => flip(n.repos, k, on))))}
                />
                <h3 class="move-sub">Boards</h3>
                <PickList
                  label="Boards"
                  data-move-boards
                  groups={[
                    {
                      key: "boards",
                      items: boardRows.map((b): Pick => ({
                        key: b.id,
                        title: b.title,
                        detail: `${b.rows} ${b.rows === 1 ? "row" : "rows"}${b.closed ? " · closed" : ""}`,
                        on: picks.boards.has(b.id),
                        why: b.repo && picks.repos.has(b.repo) ? `with ${b.repo}` : null,
                      })),
                    },
                  ]}
                  // A board with a repo brings its repo; one without moves on its own.
                  toggle={(k, on) =>
                    set((n) => {
                      const b = src.boards.find((x) => x.id === k)!;
                      flip(b.repo ? n.repos : n.boards, b.repo ?? b.id, on);
                    })
                  }
                  search={searchOver(src.boards.length, q.boards, (v) => setQ({ ...q, boards: v }), "Search boards", boardRows.filter((b) => !b.repo).map((b) => b.id), picks.boards, (keys, on) => set((n) => keys.forEach((k) => flip(n.boards, k, on))))}
                />
                <h3 class="move-sub">Docs</h3>
                <PickList
                  label="Docs"
                  data-move-docs
                  groups={docGroups(p.id, src.boards, docs ?? [], out!, moving, picks, q.docs, here)}
                  toggle={(k, on) => set((n) => flip(k.includes("/") ? n.machineDocs : n.docs, k, on))}
                  search={(() => {
                    const keys = (docs ?? []).filter((d) => !onMoving(d.id, src.boards, moving) && matches(d.title, q.docs)).map((d) => d.id);
                    return {
                      query: q.docs,
                      setQuery: (v: string) => setQ({ ...q, docs: v }),
                      placeholder: "Search Docs",
                      matching: keys.length,
                      all: () => set((n) => keys.forEach((k) => n.docs.add(k))),
                      clear: () => set((n) => n.docs.clear()),
                      picked: picks.docs.size,
                    };
                  })()}
                />
                {src.issues.length > 0 && (
                  <>
                    <h3 class="move-sub">Other issues</h3>
                    <PickList
                      label="Other issues"
                      data-move-issues
                      groups={[
                        {
                          key: "issues",
                          items: src.issues.filter((i) => matches(i.title, q.issues)).map((i) => ({ key: i.id, title: `#${i.number} ${i.title}`, detail: i.closed ? "Closed" : null, on: picks.issues.has(i.id) })),
                        },
                      ]}
                      toggle={(k, on) => set((n) => flip(n.issues, k, on))}
                      search={searchOver(
                        src.issues.length,
                        q.issues,
                        (v) => setQ({ ...q, issues: v }),
                        "Search issues",
                        src.issues.filter((i) => matches(i.title, q.issues)).map((i) => i.id),
                        picks.issues,
                        (keys, on) => set((n) => keys.forEach((k) => flip(n.issues, k, on))),
                      )}
                    />
                  </>
                )}
              </>
            )}
          </Section>
        </div>
        <div class="move-col" data-move-right>
          {said && (
            <p class="move-said" role="status" data-move-changed>
              {said}
            </p>
          )}
          {out && out.notes.length > 0 && (
            <Section id="move-check" title="Before you move">
              <div class="move-notes">
                {out.notes.map((n) => (
                  <Card key={n.key} tone="warn" data-move-note={n.key}>
                    {n.text}
                  </Card>
                ))}
              </div>
            </Section>
          )}
          <Section id="move-along" title="Comes along">
            {!along || out.empty ? (
              <p class="dim">Nothing yet.</p>
            ) : (
              <div data-move-along>
                <CountList n={along.repos.count} label={plural(along.repos.count, "repo")} rows={along.repos.items.map((r) => ({ key: String(r.id), title: r.name }))} more={() => more("repos")} data-move-along-repos />
                <CountList
                  n={along.boards.count}
                  label={plural(along.boards.count, "board")}
                  rows={along.boards.items.map((b) => ({ key: b.id, title: b.title, meta: `${b.rows} ${b.rows === 1 ? "row" : "rows"}` }))}
                  more={() => more("boards")}
                  data-move-along-boards
                />
                <CountList
                  n={along.issues.count}
                  label={plural(along.issues.count, "issue")}
                  note="Old links still work."
                  rows={along.issues.items.map((r) => ({
                    key: r.id,
                    title: r.closed ? `${r.title} (closed)` : r.title,
                    meta: (
                      <>
                        #{r.number} <ArrowIcon size="xs" /> #{r.to}
                      </>
                    ),
                  }))}
                  more={() => more("issues")}
                  data-move-along-issues
                />
                <CountList
                  n={along.github.issues + along.github.prs}
                  label="GitHub issues and pull requests"
                  rows={along.github.repos.map((r) => ({ key: r.repo, title: r.repo, meta: String(r.issues + r.prs) }))}
                  data-move-along-github
                />
                <CountList n={along.docs.count} label={plural(along.docs.count, "Doc")} rows={along.docs.items.map((d) => ({ key: d.id, title: d.title }))} more={() => more("docs")} data-move-along-docs />
                <CountList
                  n={along.workspaces.length}
                  label={plural(along.workspaces.length, "cloud workspace")}
                  rows={along.workspaces.map((w) => ({ key: w.id, title: w.name, meta: plural(w.runs, "run", true) }))}
                  data-move-along-workspaces
                />
                <CountList
                  n={along.environments.length}
                  label={plural(along.environments.length, "environment")}
                  rows={along.environments.map((e) => ({ key: e.id, title: e.name, meta: plural(e.deploys, "deploy", true) }))}
                  data-move-along-environments
                />
                <CountList n={along.sessions} label={plural(along.sessions, "session")} data-move-along-sessions />
                <CountList n={along.drafts} label={`${plural(along.drafts, "draft")} waiting`} data-move-along-drafts />
              </div>
            )}
            <div class="move-go">
              <button type="button" class="primary" disabled={busy || !out || out.empty || (isNew && !picks.name.trim())} data-move-go onClick={() => void moveIt()}>
                {busy ? "Moving…" : `Move to ${to}`}
              </button>
            </div>
            {err && (
              <p class="control-error" role="alert" data-move-error>
                {err}
              </p>
            )}
          </Section>
        </div>
      </div>
    </section>
  );
}

const plural = (n: number, word: string, withN = false) => `${withN ? `${n} ` : ""}${word}${n === 1 ? "" : "s"}`;

function repoDetail(r: Plan["source"]["repos"][number]): string | null {
  const bits = [r.boards && plural(r.boards, "board", true), r.issues && plural(r.issues, "issue", true), r.github && `${r.github} on GitHub`, r.sessions && plural(r.sessions, "session", true)].filter(Boolean);
  return bits.length ? bits.join(" · ") : null;
}

/** The search a list gets once it's long: its own query, and ticking (or
 * clearing) the pickable rows it leaves. */
function searchOver(rows: number, query: string, setQuery: (q: string) => void, placeholder: string, keys: string[], picked: Set<string>, tick: (keys: string[], on: boolean) => void) {
  if (rows < SEARCH_FROM) return undefined;
  return { query, setQuery, placeholder, matching: keys.length, all: () => tick(keys, true), clear: () => tick([...picked], false), picked: picked.size };
}

/** Whether Doc `id` is on a board that moves, so it comes along. */
const onMoving = (id: string, boards: Plan["source"]["boards"], moving: Set<string>) => boards.some((b) => moving.has(b.id) && b.docs.includes(id));

/** The Docs, in groups: the project's by the board they're on (the first,
 * with no board last), then each machine's that a moving board links, which
 * only that machine moves. */
function docGroups(project: string, boards: Plan["source"]["boards"], docs: ProjectDoc[], out: Plan, moving: Set<string>, picks: Picks, q: string, here: (machine: string) => boolean): PickGroup[] {
  const shown = docs.filter((d) => matches(d.title, q));
  const placed = new Set<string>();
  const pick = (d: ProjectDoc): Pick => ({ key: d.id, title: d.title, on: picks.docs.has(d.id), why: onMoving(d.id, boards, moving) ? "with its board" : null });
  const groups: PickGroup[] = [];
  for (const b of boards) {
    const items = shown.filter((d) => b.docs.includes(d.id) && !placed.has(d.id));
    items.forEach((d) => placed.add(d.id));
    if (items.length) groups.push({ key: b.id, title: b.title, items: items.map(pick) });
  }
  const loose = shown.filter((d) => !placed.has(d.id));
  if (loose.length) groups.push({ key: "loose", title: "Not on a board", items: loose.map(pick) });
  for (const m of out.along.machine_docs) {
    const items = m.docs.filter((d) => matches(d.title, q));
    if (!items.length) continue;
    const mine = here(m.machine);
    const at = pathOf({ page: "machines", project, at: { at: "machine", machine: m.machine } }, names);
    groups.push({
      key: `machine-${m.machine}`,
      title: `On ${m.name}`,
      note: mine ? null : `These move from ${m.name}.`,
      action: mine ? null : (
        <a class="link" href={at} data-move-open-on={m.machine} onClick={(e) => follow(e, at)}>
          Open there
        </a>
      ),
      items: items.map((d): Pick => {
        const k = `${m.machine}/${d.id}`;
        return mine ? { key: k, title: d.title, on: picks.machineDocs.has(k) } : { key: k, title: d.title, on: false, fixed: true };
      }),
    });
  }
  return groups;
}
