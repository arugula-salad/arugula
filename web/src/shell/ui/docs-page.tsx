// The Docs page (#1494, on the list page since Arugula #589): every Doc this
// machine keeps, the ones an agent wrote and left (#1493), newest first. A
// Doc outlives its pane, so closing the pane isn't losing it: Open shows it
// again in the current session (an open one comes forward), with its content
// and, if it was shared with a link, its share menu. Delete removes it for
// good, and a shared one unshares first; with control out of reach the machine
// refuses, and the reason is shown here. With none yet, it offers Start an
// agent, with the Doc line of *What your agent can do here* and its example.
//
// A row's mark is the Doc's shape: a segment a section, as long as the section
// is, brighter where it has a figure, and the whole as long as the Doc is. So a
// note and a long plan look different without opening either.

import { useEffect, useMemo, useRef, useState } from "preact/hooks";
import type { Client } from "../client";
import { opPath, OPS } from "../ops";
import type { DocLinkUrl, DocPreview, DocRow, DocSectionShape } from "../../shared/proto.gen";
import { ConfirmRemove } from "../../shared/ui/confirm";
import { ListCounts, ListPage, ListPreview, type ListCount, type ListGroup } from "../../shared/ui/list-page";
import { openMenu } from "../../shared/ui/menu";
import { PageHeader } from "../../shared/ui/page-header";
import { Time } from "../../shared/ui/time";
import { AppBar } from "./app-bar";
import { directory } from "../hosts";
import { closeDocsPage, docsRoute, openDoc } from "./docs-route";
import { startAgent } from "./agent-dialog";
import { AgentCanCard } from "./agent-can";
import { LinkIcon } from "./shell/icons";

/** The page, while the route says so and this is the machine's owner. */
export function DocsPage({ client }: { client: Client }) {
  const [, setTick] = useState(0);
  useEffect(() => {
    const fn = () => setTick((t) => t + 1);
    window.addEventListener("hashchange", fn);
    return () => window.removeEventListener("hashchange", fn);
  }, []);
  if (!docsRoute() || client.state?.roles) return null;
  return <Page client={client} />;
}

/** Who made it: an agent (`mcp:NAME`), a person's driver, a Doc from
 * before the library (`migrated`), or `unknown` (a block opened by hand). */
function madeBy(by: string): string {
  if (by === "migrated") return "made before Docs were kept";
  if (by === "unknown") return "made by someone";
  return `made by ${by.replace(/^mcp:/, "")}`;
}

const wordsOf = (r: DocRow) => r.sections.reduce((n, s) => n + s.words, 0);
const figuresOf = (r: DocRow) => r.sections.reduce((n, s) => n + s.figures, 0);
const isOpen = (r: DocRow) => r.open !== null && r.open !== undefined;
const plural = (n: number, one: string) => `${n} ${one}${n === 1 ? "" : "s"}`;
/** A Doc as long as this is as long as a mark gets. */
const LONG_DOC = 20_000;
const MARK_W = 130;
const READ_WPM = 230;

/** The Doc's shape: see the top of this file. */
function Shape({ sections }: { sections: DocSectionShape[] }) {
  if (!sections.length) return null;
  const words = sections.reduce((n, s) => n + s.words, 0);
  const width = Math.max(26, Math.sqrt(Math.min(words, LONG_DOC) / LONG_DOC) * MARK_W);
  return (
    <span class="doc-shape" style={{ width: `${Math.round(width)}px` }} aria-hidden="true" data-doc-shape>
      {sections.map((s, i) => (
        <i key={i} class={s.figures ? "fig" : undefined} style={{ flex: `${Math.max(s.words, words * 0.012, 1)} 0 0` }} />
      ))}
    </span>
  );
}

/** Which of the page's groups a Doc falls in, by the day it was last changed. */
function ageGroup(updated: number, now: number): string {
  const day = new Date(now);
  day.setHours(0, 0, 0, 0);
  const today = day.getTime();
  if (updated >= today) return "Today";
  day.setDate(day.getDate() - 1);
  if (updated >= day.getTime()) return "Yesterday";
  day.setDate(day.getDate() - 5);
  if (updated >= day.getTime()) return "This week";
  return "Earlier";
}

function Page({ client }: { client: Client }) {
  const [rows, setRows] = useState<DocRow[] | null>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [deleting, setDeleting] = useState<DocRow | null>(null);
  const [filter, setFilter] = useState("all");
  const [query, setQuery] = useState("");
  /** The openings read so far, by Doc and when it last changed. */
  const openings = useRef(new Map<string, string[]>());

  const load = async () => {
    try {
      const res = await client.request(OPS["docs.list"].method, opPath("docs.list"));
      const v = await res.json<DocRow[] & { error?: string }>().catch(() => null);
      if (!res.ok || !Array.isArray(v)) throw new Error(v?.error ?? `HTTP ${res.status}`);
      // Control serves its newest page to every machine, and a daemon before
      // 0.66.12 sends neither of these.
      setRows(v.map((r) => ({ ...r, summary: r.summary ?? "", sections: r.sections ?? [] })));
    } catch (e) {
      setError((e as Error).message);
    }
  };
  useEffect(() => void load(), []);

  const open = async (r: DocRow) => {
    setBusy(true);
    const why = await openDoc(client, r.id);
    if (why) setError(why);
    setBusy(false);
  };

  const remove = async (r: DocRow) => {
    setDeleting(null);
    setBusy(true);
    try {
      const res = await client.request(OPS["doc.delete"].method, opPath("doc.delete", { id: r.id }));
      if (!res.ok) throw new Error(((await res.json<{ error?: string }>().catch(() => null))?.error) ?? `HTTP ${res.status}`);
      setError("");
      await load();
    } catch (e) {
      // A shared Doc that control couldn't unshare says so, and stays.
      setError((e as Error).message);
    }
    setBusy(false);
  };

  const all = rows ?? [];
  const q = query.trim().toLowerCase();
  const shown = useMemo(
    () =>
      all.filter(
        (r) =>
          (filter === "all" || (filter === "open" && isOpen(r)) || (filter === "shared" && !!r.shared)) &&
          (!q || r.title.toLowerCase().includes(q) || r.summary.toLowerCase().includes(q) || r.sections.some((s) => s.heading.toLowerCase().includes(q))),
      ),
    [rows, filter, q],
  );
  const groups = useMemo(() => {
    const now = Date.now();
    const out: ListGroup<DocRow>[] = [];
    for (const r of shown) {
      const title = ageGroup(r.updated, now);
      const last = out[out.length - 1];
      if (last?.title === title) last.items.push(r);
      else out.push({ title, items: [r] });
    }
    return out;
  }, [shown]);

  const words = all.reduce((n, r) => n + wordsOf(r), 0);
  const counts: ListCount[] = [
    { key: "all", n: all.length, label: all.length === 1 ? "Doc" : "Docs" },
    { key: "open", n: all.filter(isOpen).length, label: "open now", tone: "live" },
    { key: "shared", n: all.filter((r) => !!r.shared).length, label: "shared" },
    { n: words < 1000 ? words : `${Math.round(words / 1000)}k`, label: "words" },
  ];
  const narrowed = filter !== "all" || !!q;

  const status =
    rows === null && !error ? (
      <p class="dim list-status" data-docs-loading>
        Reading this machine's Docs…
      </p>
    ) : rows?.length === 0 ? (
      <div class="agents-empty list-status" data-docs-empty>
        <b>Docs your agents write show up here.</b>
        <p>Ask any agent for a Doc (a write-up, a plan, a comparison) and it keeps one here.</p>
        <AgentCanCard client={client} only="doc" />
        <button
          class="primary"
          data-docs-start-agent
          onClick={() => {
            closeDocsPage();
            startAgent(client, { session: client.session ?? undefined });
          }}
        >
          Start an agent
        </button>
      </div>
    ) : rows && shown.length === 0 && narrowed ? (
      <p class="dim list-status" data-docs-none>
        {q ? "No Doc's title, opening or headings match." : filter === "open" ? "No Doc is open in a pane." : "No Doc is shared."}
      </p>
    ) : undefined;

  return (
    <div class="chat agents-page docs-page" data-docs-page>
      <AppBar client={client} at="docs" />
      <ListPage<DocRow>
        label="Docs"
        splitKey="docs"
        header={
          <PageHeader
            title="Docs"
            state={
              <span class="list-where" data-docs-machine>
                on {directory.current || "this machine"}
              </span>
            }
            stats={all.length > 0 && <ListCounts counts={counts} on={filter} pick={(k) => setFilter(k === filter ? "all" : k)} />}
          />
        }
        searchLabel="Search titles and headings"
        query={query}
        setQuery={setQuery}
        groups={groups}
        keyOf={(r) => r.id}
        onOpen={(r) => void open(r)}
        notice={
          error && (
            <p class="control-error agents-error" role="alert" data-docs-error>
              {error}
            </p>
          )
        }
        status={status}
        row={(r) => ({
          tone: isOpen(r) ? "live" : undefined,
          title: <span data-doc-title>{r.title}</span>,
          tags: (
            <>
              {isOpen(r) && (
                <span class="list-tag live" data-doc-open-now title="Open in a pane now">
                  %{r.open}
                </span>
              )}
              {r.shared && (
                <span class="list-tag info" data-doc-shared title="Shared with a link">
                  <LinkIcon /> shared
                </span>
              )}
            </>
          ),
          age: <Time at={r.updated} title="Updated" />,
          detail: r.summary,
          mark: <Shape sections={r.sections} />,
          attrs: { "data-doc": r.id, "data-shared": String(!!r.shared), "data-open": r.open?.toString() ?? "" },
        })}
        preview={(r) => <Preview key={r.id} client={client} doc={r} busy={busy} openings={openings.current} open={() => void open(r)} ask={() => setDeleting(r)} fail={setError} />}
      />
      {deleting && (
        <ConfirmRemove title={`Delete “${deleting.title}”?`} label="Delete" cancel={() => setDeleting(null)} go={() => void remove(deleting)}>
          <p>The Doc and its pictures are removed for good.</p>
          {deleting.shared && <p data-delete-shared>Its link stops working.</p>}
        </ConfirmRemove>
      )}
    </div>
  );
}

/** The selected Doc: what it is, its opening and its outline, and the page's one primary button. */
function Preview({ client, doc, busy, openings, open, ask, fail }: { client: Client; doc: DocRow; busy: boolean; openings: Map<string, string[]>; open: () => void; ask: () => void; fail: (why: string) => void }) {
  const key = `${doc.id}:${doc.updated}`;
  const [opening, setOpening] = useState<string[] | null>(openings.get(key) ?? null);
  const [copied, setCopied] = useState(false);
  useEffect(() => {
    if (openings.has(key)) return;
    let live = true;
    void (async () => {
      const res = await client.request(OPS["doc.preview"].method, opPath("doc.preview", { id: doc.id }));
      const v = res.ok ? await res.json<DocPreview>().catch(() => null) : null;
      if (v) openings.set(key, v.opening);
      if (live) setOpening(v?.opening ?? []);
    })().catch(() => live && setOpening([]));
    return () => void (live = false);
  }, [key]);

  const copyLink = async () => {
    try {
      const res = await client.request(OPS["doc_link.get"].method, opPath("doc_link.get", { id: doc.shared!.link }));
      const v = await res.json<Partial<DocLinkUrl> & { error?: string }>().catch(() => null);
      if (!res.ok || !v?.url) throw new Error(v?.error ?? `HTTP ${res.status}`);
      await navigator.clipboard.writeText(v.url);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch (e) {
      fail((e as Error).message);
    }
  };

  const words = wordsOf(doc);
  const figures = figuresOf(doc);
  const biggest = Math.max(1, ...doc.sections.map((s) => s.words));
  return (
    <ListPreview
      data-doc-preview={doc.id}
      title={doc.title}
      facts={
        <>
          {doc.sections.length > 0 && <span>{plural(doc.sections.length, "section")}</span>}
          {words > 0 && (
            <span>
              {plural(words, "word")} · {Math.max(1, Math.round(words / READ_WPM))} min
            </span>
          )}
          {figures > 0 && <span>{plural(figures, "figure")}</span>}
          <span>
            updated <Time at={doc.updated} on />
          </span>
          <span>{madeBy(doc.by)}</span>
        </>
      }
      actions={
        <>
          <button class="primary small" data-open-doc disabled={busy} title={isOpen(doc) ? "It's open: bring it forward" : "Show it in this session"} onClick={open}>
            {isOpen(doc) ? `Bring forward %${doc.open}` : "Open beside"}
          </button>
          {doc.shared?.link && (
            <button class="secondary small" data-doc-copy-link onClick={() => void copyLink()}>
              {copied ? "Copied" : "Copy link"}
            </button>
          )}
          <button
            class="secondary small"
            aria-label={`More for ${doc.title}`}
            title="More"
            data-doc-more
            disabled={busy}
            onClick={(e) => {
              const b = (e.currentTarget as HTMLElement).getBoundingClientRect();
              openMenu({ clientX: b.left, clientY: b.bottom + 4, preventDefault: () => e.preventDefault() }, [{ label: "Delete…", danger: true, hint: "Delete it for good", run: ask }], e.currentTarget as HTMLElement);
            }}
          >
            ⋯
          </button>
          <span class="list-preview-hint" aria-hidden="true">
            <kbd>↵</kbd> open <kbd>↑</kbd>
            <kbd>↓</kbd> move
          </span>
        </>
      }
      aside={
        doc.sections.length > 0 && (
          <>
            <h3>Outline</h3>
            <ol class="doc-outline" data-doc-outline>
              {doc.sections.map((s, i) => (
                <li key={i}>
                  <span title={s.heading}>{s.heading}</span>
                  <i class={s.figures ? "fig" : undefined} style={{ width: `${Math.max(6, (s.words / biggest) * 100)}%` }} />
                </li>
              ))}
            </ol>
          </>
        )
      }
    >
      {opening === null ? <p class="dim">Reading…</p> : opening.length ? opening.map((p, i) => <p key={i}>{p}</p>) : <p class="dim">Nothing written yet.</p>}
    </ListPreview>
  );
}
