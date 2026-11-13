// "Move to project…" (Arugula #971), as a spike: the screen from the Doc's
// design, drawn by the app's own pieces over fixture data (./fixture), with
// no move behind it. Its target is a new project (a name and an owner) or one
// that exists; on the left, what moves (repos first, then boards, Docs and
// loose issues); on the right, what to know before moving, then what comes
// along, as counts that open. No options: each case does one thing (./plan).
//
// Only a dev server draws it, at /design/move (../page.tsx). The strip at
// the top is the spike's, not the screen's: the Doc's two jobs in one click,
// the data at Aaron's size or ten times it, and whether the page is open
// on the Mac whose library has the Docs.

import { useEffect, useMemo, useState } from "preact/hooks";
import { apply as applyAppearance, legible, PALETTES } from "../../shared/appearance";
import { Bar } from "../../shared/ui/bar";
import { Card } from "../../shared/ui/card";
import { ArrowIcon } from "../../shared/ui/icons";
import { PageHeader } from "../../shared/ui/page-header";
import { CountList, PickList, type Pick, type PickGroup } from "../../shared/ui/pick-list";
import { Section } from "../../shared/ui/settings-section";
import { OWNERS, SOURCES, TARGETS, THIS_MAC, type Doc, type Source } from "./fixture";
import { boardMoves, matches, noPicks, pickable, plan, why, type Picks } from "./plan";
import "./move.css";

/** A list longer than this gets a search (as a menu of 13 rows gets a filter). */
const SEARCH_FROM = 13;

const FACTORIO = ["bradbury", "factorio-companion-bridge", "factorio-companion-mod", "factorio-mcp-server", "freight-gantries"];

export function MovePage() {
  const [sourceKey, setSourceKey] = useState(SOURCES[0].key);
  const s = SOURCES.find((x) => x.key === sourceKey)!;
  const [onMac, setOnMac] = useState(true);
  const here = onMac ? THIS_MAC : null;
  const [p, setP] = useState<Picks>(noPicks);
  const [q, setQ] = useState({ repos: "", boards: "", docs: "", issues: "" });
  const [moved, setMoved] = useState<string | null>(null);
  const out = useMemo(() => plan(s, p, here, TARGETS, OWNERS), [s, p, here]);

  const set = (f: (n: Picks) => void) =>
    setP((old) => {
      const n = { ...old, repos: new Set(old.repos), boards: new Set(old.boards), issues: new Set(old.issues), docs: new Set(old.docs) };
      f(n);
      return n;
    });
  const flip = (which: Set<string>, key: string, on: boolean) => void (on ? which.add(key) : which.delete(key));
  const restart = (src = s) => {
    setSourceKey(src.key);
    setP(noPicks());
    setQ({ repos: "", boards: "", docs: "", issues: "" });
    setMoved(null);
  };

  const tryFactorio = () => {
    restart(SOURCES[0]);
    setP({ ...noPicks(), name: "Factorio", repos: new Set(SOURCES[0].repos.filter((r) => FACTORIO.includes(r.name.split("/")[1])).map((r) => r.id)) });
  };
  const tryDocs = () => {
    restart(SOURCES[0]);
    setOnMac(true);
    setQ({ repos: "", boards: "", docs: "", issues: "" });
    setP({ ...noPicks(), target: TARGETS[0].id, docs: new Set(SOURCES[0].docs.filter((d) => pickable(d, THIS_MAC)).map((d) => d.id)) });
  };

  const [scheme, setScheme] = useState<"slate" | "latte" | null>(null);
  // As the design reference does: this page only, and leaving puts the person's own back.
  const paint = (id: "slate" | "latte") => {
    const pal = PALETTES.find((x) => x.id === id)!;
    const root = document.documentElement;
    for (const [k, v] of Object.entries(legible(pal))) root.style.setProperty(`--${k}`, v);
    root.style.colorScheme = pal.light ? "light" : "dark";
    root.dataset.scheme = pal.light ? "light" : "dark";
    root.dataset.palette = pal.id;
    setScheme(id);
  };
  useEffect(() => () => applyAppearance(), []);

  const isNew = p.target === "new";
  return (
    <div class="platform" data-platform data-move-spike>
      <main class="platform-main">
        <Bar
          class="move-try"
          title={<span class="dim">Spike</span>}
          actions={
            <>
              <button type="button" class="secondary small" data-try="factorio" onClick={tryFactorio}>
                Factorio → new project
              </button>
              <button type="button" class="secondary small" data-try="docs" onClick={tryDocs}>
                Arugula Docs → Arugula
              </button>
              <button type="button" class="secondary small" onClick={() => restart()}>
                Start over
              </button>
              <div class="seg small" role="group" aria-label="Data">
                {SOURCES.map((x) => (
                  <button key={x.key} type="button" aria-pressed={x.key === s.key} onClick={() => restart(x)}>
                    {x.label}
                  </button>
                ))}
              </div>
              <div class="seg small" role="group" aria-label="Palette">
                {(["slate", "latte"] as const).map((id) => (
                  <button key={id} type="button" aria-pressed={scheme === id} data-scheme={id} onClick={() => paint(id)}>
                    {id === "slate" ? "Slate" : "Latte"}
                  </button>
                ))}
              </div>
              <div class="seg small" role="group" aria-label="Open on">
                <button type="button" aria-pressed={onMac} onClick={() => setOnMac(true)}>
                  On {THIS_MAC}
                </button>
                <button type="button" aria-pressed={!onMac} onClick={() => setOnMac(false)}>
                  Elsewhere
                </button>
              </div>
            </>
          }
        />
        <section class="platform-section sw-page move-page">
          <PageHeader
            title="Move to project"
            back={{ href: "#", label: `${s.project.name} settings`, onClick: (e) => e.preventDefault() }}
            sub={`From ${s.project.name}. Work goes with its repo: a repo brings its boards, issues, sessions and runs.`}
          />
          <div class="move-cols">
            <div class="move-col" data-move-left>
              <Section id="move-to" title="To">
                <div class="move-fields">
                  <label>
                    Project
                    <select class="field" value={p.target} data-move-target onChange={(e) => set((n) => void (n.target = e.currentTarget.value))}>
                      <option value="new">A new project</option>
                      {TARGETS.map((t) => (
                        <option key={t.id} value={t.id}>
                          {t.name} ({t.owner})
                        </option>
                      ))}
                    </select>
                  </label>
                  {isNew && (
                    <>
                      <label>
                        Name
                        <input class="field" value={p.name} placeholder="Factorio" maxLength={100} data-move-name onInput={(e) => set((n) => void (n.name = e.currentTarget.value))} />
                      </label>
                      <label>
                        Owner
                        <select class="field" value={p.owner} data-move-owner onChange={(e) => set((n) => void (n.owner = e.currentTarget.value))}>
                          {OWNERS.map((o) => (
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
              <Section id="move-what" title="What moves" lede="Repos first: each brings its boards, its issues and the Docs on its boards.">
                <h3 class="move-sub">Repos</h3>
                <PickList
                  label="Repos"
                  data-move-repos
                  groups={[{ key: "repos", items: s.repos.filter((r) => matches(r.name, q.repos)).map((r) => repoPick(s, p, r.id)) }]}
                  toggle={(k, on) => set((n) => flip(n.repos, k, on))}
                  search={searchOver(s.repos.length, q.repos, (v) => setQ({ ...q, repos: v }), "Search repos", s.repos.filter((r) => matches(r.name, q.repos)).map((r) => r.id), p.repos, (keys, on) => set((n) => keys.forEach((k) => flip(n.repos, k, on))))}
                />
                <h3 class="move-sub">Boards</h3>
                <PickList
                  label="Boards"
                  data-move-boards
                  groups={[{ key: "boards", items: s.boards.filter((b) => matches(b.name, q.boards)).map((b) => boardPick(s, p, b.id)) }]}
                  // A board with a repo brings its repo; one without moves on its own.
                  toggle={(k, on) => set((n) => {
                    const b = s.boards.find((x) => x.id === k)!;
                    flip(b.repo ? n.repos : n.boards, b.repo ?? b.id, on);
                  })}
                  search={searchOver(s.boards.length, q.boards, (v) => setQ({ ...q, boards: v }), "Search boards", s.boards.filter((b) => !b.repo && matches(b.name, q.boards)).map((b) => b.id), p.boards, (keys, on) => set((n) => keys.forEach((k) => flip(n.boards, k, on))))}
                />
                <h3 class="move-sub">Docs</h3>
                <PickList
                  label="Docs"
                  data-move-docs
                  groups={docGroups(s, p, here, q.docs)}
                  toggle={(k, on) => set((n) => flip(n.docs, k, on))}
                  search={(() => {
                    const keys = s.docs.filter((d) => pickable(d, here) && !(d.board && boardMoves(s, p, d.board)) && matches(d.title, q.docs)).map((d) => d.id);
                    return {
                      query: q.docs,
                      setQuery: (v: string) => setQ({ ...q, docs: v }),
                      placeholder: "Search Docs",
                      matching: keys.length,
                      all: () => set((n) => keys.forEach((k) => n.docs.add(k))),
                      clear: () => set((n) => n.docs.clear()),
                      picked: p.docs.size,
                    };
                  })()}
                />
                <LooseIssues s={s} p={p} q={q.issues} setQ={(v) => setQ({ ...q, issues: v })} toggle={(k, on) => set((n) => flip(n.issues, k, on))} />
              </Section>
            </div>
            <div class="move-col" data-move-right>
              <Section id="move-check" title="Before you move">
                {out.notes.length ? (
                  <div class="move-notes">
                    {out.notes.map((n) => (
                      <Card key={n.key} tone="warn" data-move-note={n.key}>
                        {n.text}
                      </Card>
                    ))}
                  </div>
                ) : (
                  <p class="dim">{out.empty ? "Pick a repo, a board or Docs." : "Nothing to check."}</p>
                )}
              </Section>
              <Section id="move-along" title="Comes along">
                {out.empty ? (
                  <p class="dim">Nothing yet.</p>
                ) : (
                  <div data-move-along>
                    <CountList n={out.repos.length} label={plural(out.repos.length, "repo")} rows={out.repos.map((r) => ({ key: r.id, title: r.name }))} />
                    <CountList n={out.boards.length} label={plural(out.boards.length, "board")} rows={out.boards.map((b) => ({ key: b.id, title: b.name, meta: `${b.rows} rows` }))} />
                    <CountList
                      n={out.issues.length}
                      label={plural(out.issues.length, "issue")}
                      note={out.issues.length ? `They get new numbers in ${out.to}. Links to the old ones keep working.` : null}
                      rows={out.issues.map((r) => ({ key: r.issue.id, title: r.issue.closed ? `${r.issue.title} (closed)` : r.issue.title, meta: (
                          <>
                            #{r.issue.n} <ArrowIcon size="xs" /> #{r.to}
                          </>
                        ) }))}
                    />
                    {out.repos.length > 0 && (
                      <CountList n={out.github} label="issues and pull requests on GitHub" note="They follow their repos as they are." rows={out.repos.filter((r) => r.github).map((r) => ({ key: r.id, title: r.name, meta: String(r.github) }))} />
                    )}
                    <CountList n={out.docs.length} label={plural(out.docs.length, "Doc")} rows={out.docs.map((d) => ({ key: d.id, title: d.title, meta: d.machine && d.machine !== here ? `on ${d.machine}` : undefined }))} />
                    {out.repos.length > 0 && <p class="dim move-also">Sessions, runs and deploys in these repos come along too.</p>}
                  </div>
                )}
                <div class="move-go">
                  <button type="button" class="primary" disabled={out.empty || (isNew && !p.name.trim())} data-move-go onClick={() => setMoved(out.to)}>
                    Move to {out.to}
                  </button>
                </div>
                {moved && (
                  <Card tone="ok" data-move-done>
                    <p>
                      Moved to <b>{moved}</b>: {plural(out.repos.length, "repo", true)}, {plural(out.boards.length, "board", true)}, {plural(out.issues.length, "issue", true)} and {plural(out.docs.length, "Doc", true)}.
                    </p>
                    <p class="dim">
                      {s.project.name} and {moved} both say what moved, with Undo for 7 days. (The spike moves nothing.)
                    </p>
                  </Card>
                )}
              </Section>
            </div>
          </div>
        </section>
      </main>
    </div>
  );
}

const plural = (n: number, word: string, withN = false) => `${withN ? `${n} ` : ""}${word}${n === 1 ? "" : "s"}`;

/** The search a list gets once it's long: its own query, and ticking (or
 * clearing) the pickable rows it leaves. */
function searchOver(rows: number, query: string, setQuery: (q: string) => void, placeholder: string, keys: string[], picked: Set<string>, tick: (keys: string[], on: boolean) => void) {
  if (rows < SEARCH_FROM) return undefined;
  return { query, setQuery, placeholder, matching: keys.length, all: () => tick(keys, true), clear: () => tick([...picked], false), picked: picked.size };
}

function repoPick(s: Source, p: Picks, id: string): Pick {
  const r = s.repos.find((x) => x.id === id)!;
  const boards = s.boards.filter((b) => b.repo === id).length;
  const issues = s.issues.filter((i) => i.repo === id).length;
  const bits = [boards && plural(boards, "board", true), issues && plural(issues, "issue", true), r.github && `${r.github} on GitHub`].filter(Boolean);
  return { key: id, title: r.name, detail: bits.length ? bits.join(" · ") : null, on: p.repos.has(id) };
}

function boardPick(s: Source, p: Picks, id: string): Pick {
  const b = s.boards.find((x) => x.id === id)!;
  const repo = s.repos.find((r) => r.id === b.repo);
  return {
    key: id,
    title: b.name,
    detail: repo && !p.repos.has(repo.id) ? `${b.rows} rows · brings ${repo.name}` : `${b.rows} rows`,
    on: p.boards.has(id),
    why: why(s, p, b),
  };
}

/** The Docs, in groups: the platform's by the board they're on (with no
 * board last), then each machine's library, which only that machine moves. */
function docGroups(s: Source, p: Picks, here: string | null, q: string): PickGroup[] {
  const shown = s.docs.filter((d) => matches(d.title, q));
  const platform = shown.filter((d) => !d.machine);
  const groups: PickGroup[] = [];
  for (const b of s.boards) {
    const items = platform.filter((d) => d.board === b.id);
    if (items.length) groups.push({ key: b.id, title: b.name, items: items.map((d) => docPick(s, p, d)) });
  }
  const loose = platform.filter((d) => !d.board);
  if (loose.length) groups.push({ key: "loose", title: "Not on a board", items: loose.map((d) => docPick(s, p, d)) });
  const machines = [...new Set(shown.filter((d) => d.machine).map((d) => d.machine!))].sort((a, b) => (a === here ? -1 : b === here ? 1 : a.localeCompare(b)));
  for (const m of machines) {
    const items = shown.filter((d) => d.machine === m);
    const mine = m === here;
    const local = m === THIS_MAC;
    groups.push({
      key: `machine-${m}`,
      title: mine ? `On this Mac (${m})` : `On ${m}`,
      note: mine ? "Moved from there: this Mac moves them when you press Move." : "Moved from there.",
      action: mine ? null : (
        <a class="link" href="#" data-move-open-on={m} onClick={(e) => e.preventDefault()}>
          {local ? "Open on this Mac" : `Open on ${m}`}
        </a>
      ),
      items: items.map((d): Pick => (mine ? docPick(s, p, d) : { key: d.id, title: d.title, on: false, fixed: true })),
    });
  }
  return groups;
}

function docPick(s: Source, p: Picks, d: Doc): Pick {
  return { key: d.id, title: d.title, on: p.docs.has(d.id), why: why(s, p, d) };
}

/** Issues with no repo and no board: nothing brings them, so they're picked
 * one by one. */
function LooseIssues({ s, p, q, setQ, toggle }: { s: Source; p: Picks; q: string; setQ: (q: string) => void; toggle: (key: string, on: boolean) => void }) {
  const loose = s.issues.filter((i) => !i.repo && !i.board);
  if (!loose.length) return null;
  const shown = loose.filter((i) => matches(i.title, q));
  return (
    <>
      <h3 class="move-sub">Issues on no repo or board</h3>
      <PickList
        label="Issues on no repo or board"
        data-move-issues
        groups={[{ key: "issues", items: shown.map((i) => ({ key: i.id, title: `#${i.n} ${i.title}`, detail: i.closed ? "Closed" : null, on: p.issues.has(i.id) })) }]}
        toggle={toggle}
        search={searchOver(loose.length, q, setQ, "Search issues", shown.map((i) => i.id), p.issues, (keys, on) => keys.forEach((k) => toggle(k, on)))}
      />
    </>
  );
}
