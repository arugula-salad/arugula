// The Docs page (#1494, on the list page since Arugula #589, the one Docs
// list since Arugula #864, shared/docs/docs-list.tsx). On a personal
// machine it's My Docs: every Doc this machine keeps, the ones an agent wrote
// and left (#1493). On a team's machine, the machine's Docs are the team's
// (the rule `doc new` goes by, #862), so it's the team project's, in two
// tabs: My Docs and Team Docs.
//
// A library Doc outlives its pane, so closing the pane isn't losing it: Open
// shows it again in the current session (an open one comes forward), with its
// content and, if it was shared with a link, its share menu. Delete removes it
// for good, and a shared one unshares first; with control out of reach the
// machine refuses, and the reason is shown here. Move to <team> (Arugula
// #863), when the machine knows a team, gives it to the team: it leaves the
// library, and its pane follows it. With none yet, it offers
// Start an agent, with the Doc line of *What your agent can do here* and its
// example. A team's Doc opens beside as the team Doc block does, through
// control; sharing and deleting it are on its page on the platform.
//
// The preview side is the reader (#608): picking a row reads the whole Doc
// there, drawn as the Doc block draws it, from `doc.read` and `doc.asset`,
// which open no block. Open beside is for editing or watching an agent
// fill it. The list reloads when the window is back, and a Doc whose
// `updated` moved is read again in place.

import { BlockLoading, EmptyState } from "../../shared/ui/block-states";
import { useEffect, useMemo, useRef, useState } from "preact/hooks";
import type { Client } from "../client";
import { opPath, OPS } from "../ops";
import { DocBlock, type DocState } from "../../shared/docs/doc-render";
import { shareClosedDoc } from "./doc-share";
import { DocActions, openTab } from "../../shared/docs/doc-actions";
import type { AccessView, DocAsset, DocContent, DocLinkUrl, DocMoveRequest, DocRow, DocTeam, DocTeams, PlatformProjects, ProjectDoc } from "../../shared/proto.gen";
import { ConfirmRemove } from "../../shared/ui/confirm";
import { ListCounts, ListPreview, type ListCount } from "../../shared/ui/list-page";
import { openMenu } from "../../shared/ui/menu";
import { IconButton, LinkIcon, MoreIcon } from "../../shared/ui/icons";
import { DocsList, goToSection, Outline } from "../../shared/docs/docs-list";
import { keptLine, machineMode, PERSONAL, type DocsMode } from "../../shared/docs/docs-mode";
import type { BackLink } from "../../shared/ui/page-header";
import type { ComponentChildren } from "preact";
import { noImages, projectItem, shapeOf, stateOf as projectState, useProjectDoc, useProjectDocs } from "../../shared/docs/project-docs";
import { platformFetch } from "../../shared/maintenance";
import { AppBar } from "./app-bar";
import { closeDocsPage, docsRoute, openDoc } from "./docs-route";
import { startAgent } from "./agent-dialog";
import { AgentCanCard } from "./agent-can";
import { Chip, StateChip } from "../../shared/ui/state-chip";

/** What the reader keeps of a Doc it has read, for the next time it is picked. */
interface Read {
  state: DocState | null;
  /** Its pictures by `name/rev`, each as the data: URL it is shown from. */
  images: Map<string, string>;
}

const NO_FRESH = new Map<string, number>();

/** A Doc as the Doc block's renderer takes it. */
const stateOf = (c: DocContent, row: DocRow): DocState => ({ ...c, doc: c.doc ?? undefined, error: c.error ?? undefined, made: row.made, updated: row.updated, by: row.by, layout: row.layout });

/** The page, while the route says so and this is the machine's owner. In
 * the workspace (`inWorkspace`) it sits under the workspace's header, beside
 * its rail, as the panes do (Arugula #796); elsewhere (a phone) it covers the
 * window under a bar of its own. */
export function DocsPage({ client, inWorkspace }: { client: Client; inWorkspace?: boolean }) {
  const [, setTick] = useState(0);
  useEffect(() => {
    const fn = () => setTick((t) => t + 1);
    window.addEventListener("hashchange", fn);
    return () => window.removeEventListener("hashchange", fn);
  }, []);
  if (!docsRoute() || client.state?.roles) return null;
  return <Page client={client} inWorkspace={inWorkspace} />;
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
const READ_WPM = 230;

/** Whose Docs the machine's are, and who reads them: its team's project
 * when it's a team's machine (`GET /api/access` says its team) and the
 * platform lists that project for the reader; else its library. Null while
 * it asks. A page with no platform behind it (a daemon's own) has the library. */
function useMode(client: Client): { mode: DocsMode; me: string | null } | null {
  const [got, setGot] = useState<{ mode: DocsMode; me: string | null } | null>(null);
  useEffect(() => {
    let live = true;
    void (async () => {
      try {
        const res = await client.request(OPS["access.get"].method, opPath("access.get"));
        const team = res.ok ? (await res.json<AccessView>().catch(() => null))?.team?.id : null;
        if (!team) return live && setGot({ mode: PERSONAL, me: null });
        const r = await platformFetch("/api/projects", { credentials: "same-origin" });
        const l = r.ok ? ((await r.json().catch(() => null)) as PlatformProjects | null) : null;
        if (live) setGot(l && Array.isArray(l.projects) ? { mode: machineMode(team, l.projects), me: l.me?.account ?? null } : { mode: PERSONAL, me: null });
      } catch {
        if (live) setGot({ mode: PERSONAL, me: null });
      }
    })();
    return () => void (live = false);
  }, [client]);
  return got;
}

function Page({ client, inWorkspace }: { client: Client; inWorkspace?: boolean }) {
  const got = useMode(client);
  return (
    <div class={inWorkspace ? "chat agents-page docs-page ws-page" : "chat agents-page docs-page"} data-docs-page data-docs-mode={got?.mode.kind}>
      {!inWorkspace && <AppBar client={client} at="docs" />}
      {got === null ? (
        <div class="list-status">
          <BlockLoading inline data-docs-loading what="this machine's Docs" />
        </div>
      ) : got.mode.kind === "team" ? (
        <TeamMachine client={client} mode={got.mode} me={got.me} />
      ) : (
        <Library client={client} />
      )}
    </div>
  );
}

/** A team's machine: Team Docs, and, apart from them, the library Docs it
 * still keeps (written before its Docs were the team's, #862), so they can be
 * read and moved. The two are never one list: the library is a view of its
 * own, reached from a line over Team Docs, with the way back. */
function TeamMachine({ client, mode, me }: { client: Client; mode: Extract<DocsMode, { kind: "team" }>; me: string | null }) {
  const [library, setLibrary] = useState(false);
  const [kept, setKept] = useState(0);
  useEffect(() => {
    if (library) return;
    let live = true;
    void (async () => {
      try {
        const res = await client.request(OPS["docs.list"].method, opPath("docs.list"));
        const v = res.ok ? await res.json<DocRow[]>().catch(() => null) : null;
        if (live) setKept(Array.isArray(v) ? v.length : 0);
      } catch {
        // No line, then.
      }
    })();
    return () => void (live = false);
  }, [client, library]);
  if (library)
    return (
      <Library
        client={client}
        title="On this machine"
        back={{
          href: "#docs",
          label: "Team Docs",
          data: "docs-back-team",
          onClick: (e) => {
            e.preventDefault();
            setLibrary(false);
          },
        }}
      />
    );
  const line = keptLine(kept);
  return (
    <TeamDocs
      client={client}
      mode={mode}
      me={me}
      under={
        line && (
          <p class="docs-kept dim" data-docs-kept={kept}>
            {line} ·{" "}
            <button type="button" class="link" data-docs-kept-show onClick={() => setLibrary(true)}>
              Show
            </button>
          </p>
        )
      }
    />
  );
}

/** A personal machine's Docs: its library. On a team's machine, the Docs it
 * kept from before, under `title`, with `back` to Team Docs. */
function Library({ client, title, back }: { client: Client; title?: string; back?: BackLink }) {
  const [rows, setRows] = useState<DocRow[] | null>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [deleting, setDeleting] = useState<DocRow | null>(null);
  const [moving, setMoving] = useState<DocRow | null>(null);
  /** The team a Doc here moves to, if the machine knows one. */
  const [team, setTeam] = useState<DocTeam | null>(null);
  const [filter, setFilter] = useState("all");
  /** The Docs read so far, by id. */
  const reads = useRef(new Map<string, Read>());

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
  useEffect(() => {
    void client
      .request(OPS["docs.team"].method, opPath("docs.team"))
      .then((res) => (res.ok ? res.json<DocTeams>() : null))
      .then((v) => setTeam(v?.team ?? null))
      .catch(() => {});
  }, []);
  // This list hears of nothing on its own: back in the window, it asks.
  useEffect(() => {
    window.addEventListener("focus", load);
    return () => window.removeEventListener("focus", load);
  }, []);

  const open = async (r: DocRow) => {
    setBusy(true);
    const why = await openDoc(client, r.id);
    if (why) setError(why);
    setBusy(false);
  };

  // Share goes by the Doc's id (Arugula #819): no pane opens, whether the
  // Doc is open or not. Its dialog confirms; the list reads back after.
  const share = (r: DocRow) =>
    shareClosedDoc(client, { id: r.id, shared: r.shared ?? null, older: !!r.shared && r.updated > r.shared.updated }, async (why) => {
      setError(why ?? "");
      await load();
    });

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

  const move = async (r: DocRow) => {
    setMoving(null);
    setBusy(true);
    try {
      const body = { project: team?.project ?? null } satisfies DocMoveRequest;
      const res = await client.request(OPS["doc.move"].method, opPath("doc.move", { id: r.id }), body);
      if (!res.ok) throw new Error(((await res.json<{ error?: string }>().catch(() => null))?.error) ?? `HTTP ${res.status}`);
      setError("");
      await load();
    } catch (e) {
      setError((e as Error).message);
    }
    setBusy(false);
  };

  const counts = (all: DocRow[]): ListCount[] => [
    { key: "all", n: all.length, label: all.length === 1 ? "Doc" : "Docs" },
    { key: "open", n: all.filter(isOpen).length, label: "open now", tone: "live" },
    { key: "shared", n: all.filter((r) => !!r.shared).length, label: "shared" },
  ];

  return (
    <>
      <DocsList<DocRow>
        mode={PERSONAL}
        docs={rows}
        item={(r) => ({ id: r.id, title: r.title, layout: r.layout, made: r.made, updated: r.updated, by: r.by, sections: r.sections, summary: r.summary, mine: true })}
        splitKey="docs"
        title={title}
        back={back}
        stats={(all) => all.length > 0 && <ListCounts counts={counts(all)} on={filter} pick={(k) => setFilter(k === filter ? "all" : k)} />}
        narrow={filter === "all" ? undefined : { test: (r) => (filter === "open" ? isOpen(r) : !!r.shared), none: filter === "open" ? "No Doc is open in a pane." : "No Doc is shared." }}
        loading={!error && <BlockLoading inline data-docs-loading what="this machine's Docs" />}
        empty={
          <EmptyState
            where="page"
            data-docs-empty
            title="No Docs yet."
            why="Docs your agents write show up here. Ask any agent for a Doc (a write-up, a plan, a comparison) and it keeps it here."
            action={
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
            }
          >
            <AgentCanCard client={client} only="doc" />
          </EmptyState>
        }
        notice={
          error && (
            <p class="control-error agents-error" role="alert" data-docs-error>
              {error}
            </p>
          )
        }
        onOpen={(r) => void open(r)}
        row={(r) => ({
          tone: isOpen(r) ? "live" : undefined,
          tags: (
            <>
              {isOpen(r) && (
                <StateChip tone="accent" data-doc-open-now title="Open in a pane now">
                  %{r.open}
                </StateChip>
              )}
              {r.shared && (
                <Chip kind="tag" tone="info" data-doc-shared title="Shared with a link">
                  <LinkIcon /> shared
                </Chip>
              )}
              {r.platform && (
                <Chip kind="tag" data-doc-platform title="Its copy on the platform reads with this machine asleep">
                  on the platform
                </Chip>
              )}
            </>
          ),
          attrs: { "data-shared": String(!!r.shared), "data-open": r.open?.toString() ?? "" },
        })}
        preview={(r) => <Preview key={r.id} client={client} doc={r} busy={busy} reads={reads.current} open={() => void open(r)} share={() => share(r)} ask={() => setDeleting(r)} team={team?.name} moveTo={() => setMoving(r)} fail={setError} />}
      />
      {deleting && (
        <ConfirmRemove title={`Delete “${deleting.title}”?`} label="Delete" cancel={() => setDeleting(null)} go={() => void remove(deleting)}>
          <p>The Doc and its pictures are removed for good.</p>
          {deleting.shared && <p data-delete-shared>Its link stops working.</p>}
        </ConfirmRemove>
      )}
      {moving && team && (
        <ConfirmRemove title={`Move “${moving.title}” to ${team.name}?`} label="Move" cancel={() => setMoving(null)} go={() => void move(moving)}>
          <p>{team.name} will own it: it leaves this machine's Docs, and a pane or agent showing it goes on with the team's Doc.</p>
          {moving.shared && <p data-move-shared>Its link keeps working, as it is now.</p>}
        </ConfirmRemove>
      )}
    </>
  );
}

/** A team's machine's Docs: the team project's, My Docs and Team Docs. */
function TeamDocs({ client, mode, me, under }: { client: Client; mode: Extract<DocsMode, { kind: "team" }>; me: string | null; under?: ComponentChildren }) {
  const { docs } = useProjectDocs(mode.project);
  const [error, setError] = useState("");
  // Open beside: the team Doc block, as `doc open` makes it, reading and writing through control.
  const open = async (d: ProjectDoc) => {
    const pane = await client.openBlock(
      { type: "doc", config: { doc: d.id, project: mode.project, project_name: mode.name, title: d.title }, session: client.session?.toString() ?? null, local: true },
      "couldn't open the Doc",
    );
    if (pane === null) return;
    if (docsRoute()) closeDocsPage();
    client.showWhenKnown(pane);
  };
  return (
    <DocsList<ProjectDoc>
      mode={mode}
      docs={docs}
      item={(d) => projectItem(d, me)}
      splitKey="docs"
      sub={`${mode.name}'s Docs: this is the team's machine`}
      under={under}
      stats={(all) => docs !== null && <ListCounts counts={[{ key: "all", n: all.length, label: all.length === 1 ? "Doc" : "Docs" }]} on="all" pick={() => {}} />}
      loading={<BlockLoading inline data-docs-loading what={`${mode.name}'s Docs`} />}
      empty={<EmptyState where="page" data-docs-empty title="No Docs yet." why={`Docs this machine's agents write go to ${mode.name}, and show up here.`} />}
      notice={
        error && (
          <p class="control-error agents-error" role="alert" data-docs-error>
            {error}
          </p>
        )
      }
      onOpen={(d) => void open(d)}
      preview={(d) => <TeamPreview key={d.id} project={mode.project} doc={d} open={() => void open(d)} fail={setError} />}
    />
  );
}

/** A team Doc, whole, kept live through its thread: Open beside, and its link to copy or open. */
function TeamPreview({ project, doc, open, fail }: { project: string; doc: ProjectDoc; open: () => void; fail: (why: string) => void }) {
  const view = useProjectDoc(project, doc.id);
  const state = useMemo(() => (view ? projectState(view) : null), [view]);
  const reader = useRef<HTMLDivElement>(null);
  const link = view?.link ?? null;
  const copy = async (): Promise<"ok" | false> => {
    try {
      await navigator.clipboard.writeText(link!);
      return "ok";
    } catch (e) {
      fail((e as Error).message);
      return false;
    }
  };
  return (
    <ListPreview
      data-doc-preview={doc.id}
      factsTip={`${doc.written} of ${doc.sections} written`}
      actions={
        <>
          <button class="primary small" data-open-doc title="Show it in this session" onClick={open}>
            Open beside
          </button>
          <DocActions none={!link} copy={copy} open={() => link && openTab(link)} hooks={{ copy: { "data-doc-copy-link": "" } }} />
        </>
      }
      aside={view && shapeOf(view.sections).length > 0 && <Outline sections={shapeOf(view.sections)} go={(i) => goToSection(reader.current, i)} />}
    >
      <div ref={reader} class="docs-reader" data-doc-reader>
        <DocBlock id={0} docKey={doc.id} s={state} fresh={NO_FRESH} load={noImages} />
      </div>
    </ListPreview>
  );
}

/** The selected Doc: what it is, the page's one primary button, and the Doc itself, read in place. */
function Preview({ client, doc, busy, reads, open, share, ask, team, moveTo, fail }: { client: Client; doc: DocRow; busy: boolean; reads: Map<string, Read>; open: () => void; share: () => void; ask: () => void; team?: string; moveTo: () => void; fail: (why: string) => void }) {
  const kept = useMemo(() => {
    let r = reads.get(doc.id);
    if (!r) reads.set(doc.id, (r = { state: null, images: new Map() }));
    return r;
  }, [doc.id]);
  const [state, setState] = useState<DocState | null>(kept.state);
  const reader = useRef<HTMLDivElement>(null);

  // Read it when picked and whenever it changed or the window is back. The
  // Doc on screen stays until the new one is here, and is replaced only if
  // it differs, so the scroll stays and nothing flashes.
  useEffect(() => {
    let live = true;
    let latest = 0;
    const go = async () => {
      const mine = ++latest;
      try {
        const res = await client.request(OPS["doc.read"].method, opPath("doc.read", { id: doc.id }));
        const v = res.ok ? await res.json<DocContent>().catch(() => null) : null;
        if (!v || !live || mine !== latest) return;
        const next = stateOf(v, doc);
        if (JSON.stringify(next) !== JSON.stringify(kept.state)) setState((kept.state = next));
      } catch {
        // The Doc on screen stays.
      }
    };
    void go();
    window.addEventListener("focus", go);
    return () => {
      live = false;
      window.removeEventListener("focus", go);
    };
  }, [doc.id, doc.updated]);

  // A picture, once, by `name/rev`; the name's older revs are done with.
  const load = useMemo(() => {
    const asking = new Map<string, Promise<string | null>>();
    return (name: string, rev: number) => {
      const key = `${name}/${rev}`;
      const have = kept.images.get(key);
      if (have) return Promise.resolve(have);
      let p = asking.get(key);
      if (!p) {
        p = client
          .request(OPS["doc.asset"].method, opPath("doc.asset", { id: doc.id, name }))
          .then(async (res) => {
            const v = res.ok ? await res.json<DocAsset>() : null;
            if (!v) return null;
            const url = `data:${v.mime};base64,${v.data}`;
            for (const k of kept.images.keys()) if (k.startsWith(`${name}/`)) kept.images.delete(k);
            kept.images.set(key, url);
            return url;
          })
          .catch(() => null)
          .finally(() => asking.delete(key));
        asking.set(key, p);
      }
      return p;
    };
  }, [doc.id]);

  // The one link Copy link copies and Open in browser opens (Arugula #708):
  // the public link it has (anyone can open it), else its copy on the
  // platform (only its owner can). Null, with the reason said, if it fails.
  const linkOf = async (): Promise<string | null> => {
    try {
      let url = doc.shared?.link ? undefined : doc.platform?.link;
      if (!url) {
        const res = await client.request(OPS["doc_link.get"].method, opPath("doc_link.get", { id: doc.shared!.link }));
        const v = await res.json<Partial<DocLinkUrl> & { error?: string }>().catch(() => null);
        if (!res.ok || !v?.url) throw new Error(v?.error ?? `HTTP ${res.status}`);
        url = v.url;
      }
      return url;
    } catch (e) {
      fail((e as Error).message);
      return null;
    }
  };
  const copyLink = async (): Promise<"ok" | "private" | false> => {
    const url = await linkOf();
    if (!url) return false;
    try {
      await navigator.clipboard.writeText(url);
    } catch (e) {
      fail((e as Error).message);
      return false;
    }
    return doc.platform && !doc.shared ? "private" : "ok";
  };

  const words = wordsOf(doc);
  const figures = figuresOf(doc);
  const none = !doc.platform && !doc.shared?.link;
  return (
    <ListPreview
      data-doc-preview={doc.id}
      facts={words > 0 && <span>{Math.max(1, Math.round(words / READ_WPM))} min read</span>}
      factsTip={[plural(doc.sections.length, "section"), plural(words, "word"), figures > 0 && plural(figures, "figure"), madeBy(doc.by)].filter(Boolean).join(" · ")}
      hint={
        <span class="list-preview-hint" aria-hidden="true">
          <kbd>↵</kbd> open <kbd>↑</kbd>
          <kbd>↓</kbd> move
        </span>
      }
      actions={
        <>
          <button class="primary small" data-open-doc disabled={busy} title={isOpen(doc) ? "It's open: bring it forward" : "Show it in this session"} onClick={open}>
            {isOpen(doc) ? `Bring forward %${doc.open}` : "Open beside"}
          </button>
          <DocActions
            none={none}
            copy={copyLink}
            open={() => void linkOf().then((url) => url && openTab(url))}
            share={share}
            older={!!doc.shared && doc.updated > doc.shared.updated}
            hooks={{ copy: { "data-doc-copy-link": "" }, open: { "data-doc-platform-link": doc.platform?.link } }}
          />
          <IconButton
            label={`More for ${doc.title}`}
            title="More"
            class="bar-btn sq"
            aria-haspopup="menu"
            data-doc-more
            disabled={busy}
            onClick={(e) => {
              const b = (e.currentTarget as HTMLElement).getBoundingClientRect();
              const moves = team ? [{ label: `Move to ${team}…`, hint: "The team owns it from then on", run: moveTo }] : [];
              openMenu({ clientX: b.left, clientY: b.bottom + 4, preventDefault: () => e.preventDefault() }, [...moves, { label: "Delete…", danger: true, hint: "Delete it for good", run: ask }], e.currentTarget as HTMLElement);
            }}
          >
            <MoreIcon size="sm" />
          </IconButton>
        </>
      }
      aside={doc.sections.length > 0 && <Outline sections={doc.sections} go={(i) => goToSection(reader.current, i)} />}
    >
      <div ref={reader} class="docs-reader" data-doc-reader>
        <DocBlock id={0} docKey={doc.id} s={state} fresh={NO_FRESH} load={load} cached={kept.images} />
      </div>
    </ListPreview>
  );
}
