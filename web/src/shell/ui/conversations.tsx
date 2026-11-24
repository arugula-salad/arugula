// Your work (Arugula #1396): the Claude Code and Codex conversations on the
// daemon's machine (M33), from a terminal, the desktop app's Code tab, VS
// Code or Codex, grouped by repo (their folder) and newest first, three a
// repo until "N more". A row's first line is its first prompt, in the
// person's own words, and its second the last one. Each has one button:
// Continue here, a terminal pane in its folder running the person's own
// `claude --resume <id>` (or `codex resume <id>`), in a new tab named after
// it (no adapter to install); Go to pane when it's open here already. One
// that Claude Code runs in a terminal outside Arugula on this machine asks
// first ("Running in iTerm. Move here?"): *Move here* stops it there
// (SIGINT, then SIGTERM: the daemon's `move_here`, Arugula #1399) and
// continues it here; if it won't stop, the row says to quit it there. If
// its folder is gone, the row says so and offers to run it in home. `⋯` holds *Open as agent block* (stopped, its transcript
// as it grows) and *Fork here*; a Codex one has neither.
//
// `YourWork` is the one component: the owner's home on the daemon's page
// when no session is open (ui/app.tsx), the dialog the pane menu, the
// palette, the agent dialog and the phone's sheet open (`pickConversation`),
// and the Overview's (Arugula #1400). On a phone the dialog is a
// full-height sheet.
//
// With more than one host (#78), the dialog lists every host's, grouped by
// host and then repo: each host is asked over the fleet's connection (M25)
// at once and shows up as it answers. One that's asleep isn't woken to be
// asked, and one that doesn't answer in a few seconds says so. Opening one
// of another host's opens it on that host and shows it there. The page is
// this machine's.

import { OPS, opPath } from "../ops";
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "preact/hooks";
import type { ApiResponse, Client } from "../client";
import { directory } from "../hosts";
import type { ContinueConversationRequest, ContinueConversationResponse, OpenConversationRequest, OpenConversationResponse, PaneId } from "../../shared/proto";
import { getFleet } from "./hosts";
import { countView } from "../../shared/telemetry";
import { Time } from "../../shared/ui/time";
import type { AgentContext, Estimate } from "../blocks/agent-context";
import { Modal } from "../../shared/ui/modal";
import { PageHeader } from "../../shared/ui/page-header";
import { EmptyLine, EmptyState } from "../../shared/ui/block-states";
import { openMenu, type MenuItem } from "../../shared/ui/menu";
import { CloseIcon, IconButton, MoreIcon } from "../../shared/ui/icons";
import { useSubscribe } from "../../shared/ui/hooks";
import { conversationContinued } from "./machine-telemetry";

export interface Conversation {
  id: string;
  cwd: string;
  cwd_exists: boolean;
  branch?: string;
  /** Whose it is: Codex's go on in a terminal pane, not an agent block. */
  agent?: "claude" | "codex";
  source: "terminal" | "desktop" | "vscode" | "codex" | "other";
  title: string;
  first_prompt?: string;
  last_prompt?: string;
  model?: string;
  updated_ms: number;
  forked_from?: string;
  archived: boolean;
  live?: { pid: number; pane?: PaneId; block?: PaneId; place: string; status: string; entrypoint?: string; terminal?: string };
  /** What it and its subagents cost at API list prices (#398), once the
   *  daemon has read it. */
  estimate?: Estimate;
  /** What Claude Code said of it from inside, live (#453). */
  context?: AgentContext;
  /** The agent block that has it open. */
  block: PaneId | null;
}

export interface ConversationsWhere {
  /** Split this block (desktop), else a new tab in `session`. */
  split?: PaneId;
  session?: number;
  /** Folders under this one first. */
  cwd?: string;
}

let open: { client: Client; where: ConversationsWhere; phone: boolean } | null = null;
const listeners = new Set<() => void>();
const changed = () => listeners.forEach((fn) => fn());

/** Show Your work as a dialog. */
export function pickConversation(client: Client, where: ConversationsWhere, phone = false) {
  countView("conversations");
  open = { client, where, phone };
  changed();
}

/** Your work from the palette or the machine menu: beside the active pane,
 * in its folder first. */
export function openYourWork(client: Client) {
  const pane = client.active();
  pickConversation(client, pane === undefined ? { session: client.session ?? undefined } : { split: pane, cwd: client.cwd(pane) ?? undefined });
}

export function ConversationsLayer() {
  const [, setTick] = useState(0);
  useEffect(() => {
    const fn = () => setTick((t) => t + 1);
    listeners.add(fn);
    return () => {
      listeners.delete(fn);
    };
  }, []);
  if (!open) return null;
  const { client, where, phone } = open;
  const close = () => {
    open = null;
    changed();
  };
  return (
    <Modal bare close={close} label="Your work" class={phone ? "your-work-dialog phone" : "your-work-dialog"}>
      <YourWork client={client} where={where} phone={phone} done={close} everywhere />
    </Modal>
  );
}

/** Whether the owner's empty home is Your work. Under a test runner
 * (Playwright sets `navigator.webdriver`) it's the plain empty layout, as
 * Getting started and the hints keep quiet there; a spec that wants the
 * first screen as a person sees it opts in with `__arugulaFirstRun`. */
export function yourWorkHome(): boolean {
  return !navigator.webdriver || !!(window as { __arugulaFirstRun?: boolean }).__arugulaFirstRun;
}

/** `~/dev/x` for `/home/me/dev/x` (the daemon's home, guessed from paths). */
function tilde(path: string, home: string | null): string {
  return home && (path === home || path.startsWith(home + "/")) ? "~" + path.slice(home.length) : path;
}

function homeOf(list: Conversation[]): string | null {
  for (const c of list) {
    const m = /^(\/home\/[^/]+|\/Users\/[^/]+|\/root)(\/|$)/.exec(c.cwd);
    if (m) return m[1];
  }
  return null;
}

/** A folder's name: its repo, as people call it. */
function repoName(cwd: string): string {
  return cwd.replace(/[/\\]+$/, "").split(/[/\\]/).pop() || cwd || "(no folder)";
}

/** One host's conversations, or why there are none. */
interface HostConvs {
  host: string;
  /** Asked, no answer yet; answered; asleep (not asked); or away. */
  state: "reading" | "ok" | "asleep" | "away";
  list: Conversation[];
  note?: string;
}

/** How long a host gets to answer before it's shown as not answering. */
const HOST_MS = 5000;
/** How many of a repo's show before "N more". */
const PER_REPO = 3;

function within<T>(p: Promise<T>, ms: number): Promise<T> {
  return new Promise((resolve, reject) => {
    const t = setTimeout(() => reject(new Error("not answering")), ms);
    p.then(
      (v) => (clearTimeout(t), resolve(v)),
      (e) => (clearTimeout(t), reject(e)),
    );
  });
}

interface Repo {
  host: string;
  cwd: string;
  home: string | null;
  convs: Conversation[];
}

/** One host's repos and how it answered; `repos` by their newest
 * conversation, the one we came from first. */
interface HostView {
  h: HostConvs;
  repos: Repo[];
}

type Conv = { c: Conversation; host: string };

/** Where a conversation runs that isn't here, for people: the terminal
 * app the daemon named (iTerm, Ghostty…), else what it came from. */
function elsewhere(c: Conversation): string {
  if (c.live?.terminal) return c.live.terminal;
  if (c.source === "desktop") return "the Claude desktop app";
  if (c.source === "vscode") return "VS Code";
  return "another terminal";
}

/** The machine's conversations as Your work: a page (`done` unset) or the
 * dialog's body (`done` closes it). `everywhere`: every host of a fleet's. */
export function YourWork({
  client,
  where = {},
  phone = false,
  done,
  everywhere = false,
}: {
  client: Client;
  where?: ConversationsWhere;
  phone?: boolean;
  done?: () => void;
  everywhere?: boolean;
}) {
  useSubscribe((fn) => directory.subscribe(fn));
  const dialog = !!done;
  const [hosts, setHosts] = useState<HostConvs[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  const [sel, setSel] = useState(0);
  const [busy, setBusy] = useState(false);
  /** Repos shown in full, by `host:cwd`. */
  const [more, setMore] = useState<Set<string>>(new Set());
  /** The conversation whose folder is gone, as Continue here found: offer home. */
  const [gone, setGone] = useState<{ id: string; host: string; cwd: string } | null>(null);
  /** The one Move here couldn't stop, and what the daemon said. */
  const [refused, setRefused] = useState<{ id: string; host: string; why: string } | null>(null);
  const input = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const loads = useRef(0);
  const close = () => done?.();

  // This page's host, and with a fleet the others (#78).
  const fleet = getFleet();
  const here = directory.current;
  const multi = everywhere && !!fleet && fleet.list.length > 1;

  const ask = (host: string, path: string): Promise<ApiResponse> =>
    host === here ? client.request(OPS["conversations.list"].method, path) : fleet!.request(host, OPS["conversations.list"].method, path);

  const load = () => {
    const gen = ++loads.current;
    setError(null);
    const path = `${opPath("conversations.list")}?limit=1000`;
    const others = multi ? fleet!.list.filter((h) => h.name !== here) : [];
    const start: HostConvs[] = [
      { host: here, state: "reading", list: [] },
      ...others.map((h): HostConvs => {
        if (h.state === "asleep") return { host: h.name, state: "asleep", list: [], note: "asleep" };
        if (h.state === "capped") return { host: h.name, state: "away", list: [], note: "not live (too many machines)" };
        return { host: h.name, state: "reading", list: [] };
      }),
    ];
    // A reload keeps what's shown until the answer replaces it.
    setHosts((cur) => start.map((h) => (h.state === "reading" && cur?.find((x) => x.host === h.host && x.state === "ok")) || h));
    const set = (h: HostConvs) => gen === loads.current && setHosts((cur) => cur && cur.map((x) => (x.host === h.host ? h : x)));
    for (const h of start) {
      if (h.state !== "reading") continue;
      within(ask(h.host, path), HOST_MS)
        .then(async (res) => {
          if (!res.ok) throw new Error(`couldn't list them (${res.status})`);
          set({ host: h.host, state: "ok", list: (await res.json<{ conversations: Conversation[] }>()).conversations });
        })
        .catch((e: Error) => {
          if (!multi && gen === loads.current) setError(e.message);
          const st = fleet?.host(h.host)?.state;
          const why = st === "stale" || st === "offline" ? `${st} · ${e.message}` : e.message;
          set({ host: h.host, state: "away", list: [], note: why });
        });
    }
  };
  useEffect(() => {
    load();
    // Coming back to the page: what's live may have changed.
    const back = () => document.visibilityState === "visible" && load();
    document.addEventListener("visibilitychange", back);
    return () => document.removeEventListener("visibilitychange", back);
  }, [here]);
  useLayoutEffect(() => {
    if (dialog && !phone) input.current?.focus();
  }, []);

  // Each host's repos by their newest conversation; this host first, and
  // in it the folder we came from.
  const view: HostView[] = useMemo(() => {
    if (!hosts) return [];
    const words = query.toLowerCase().split(/\s+/).filter(Boolean);
    return hosts.map((h) => {
      const home = homeOf(h.list);
      const hits = h.list.filter((c) => {
        const hay = `${c.title} ${c.first_prompt ?? ""} ${c.last_prompt ?? ""} ${c.cwd}`.toLowerCase();
        return words.every((w) => hay.includes(w));
      });
      const groups = new Map<string, Conversation[]>();
      for (const c of [...hits].sort((a, b) => b.updated_ms - a.updated_ms)) {
        const g = groups.get(c.cwd) ?? [];
        g.push(c);
        groups.set(c.cwd, g);
      }
      const cwd = h.host === here ? where.cwd : undefined;
      const near = (a: string) => (cwd && (cwd === a || cwd.startsWith(a + "/") || a.startsWith(cwd + "/")) ? 1 : 0);
      const order = [...groups.keys()].sort((a, b) => {
        if (near(a) !== near(b)) return near(b) - near(a);
        return groups.get(b)![0].updated_ms - groups.get(a)![0].updated_ms;
      });
      return { h, repos: order.map((f) => ({ host: h.host, cwd: f, home, convs: groups.get(f)! })) };
    });
  }, [hosts, query, where.cwd, here]);
  /** A repo's rows as shown: three, or all of them once asked, or while searching. */
  const shown = (r: Repo) => (query || more.has(`${r.host}:${r.cwd}`) ? r.convs : r.convs.slice(0, PER_REPO));
  const convs: Conv[] = view.flatMap((v) => v.repos.flatMap((r) => shown(r).map((c) => ({ c, host: r.host }))));
  const reading = !!hosts && hosts.some((h) => h.state === "reading");
  const total = hosts?.reduce((n, h) => n + h.list.length, 0) ?? 0;

  useEffect(() => {
    if (sel >= convs.length && convs.length > 0) setSel(convs.length - 1);
  }, [convs.length]);
  useEffect(() => {
    listRef.current?.querySelector(".yw-row.selected")?.scrollIntoView({ block: "nearest" });
  }, [sel]);

  /** The pane it runs in, if that's one of its host's panes. */
  const inPane = (c: Conversation, host: string): boolean => {
    const p = c.live?.pane;
    if (p === undefined) return false;
    if (host === here) return !!client.info(p);
    return !!fleet?.panes.some((x) => x.host === host && x.id === p && !x.stale);
  };
  /** Open here already: in a pane or an agent block. */
  const openHere = (c: Conversation, host: string) => c.block !== null || inPane(c, host);
  /** Running somewhere Arugula doesn't show: another terminal, the desktop app. */
  const runsElsewhere = (c: Conversation, host: string) => !!c.live && !openHere(c, host);
  /** Running at a terminal outside Arugula on this machine: Move here can
   * stop it there and go on with it here. Never another host's. */
  const movable = (c: Conversation, host: string): boolean =>
    host === here && c.agent !== "codex" && c.live?.entrypoint === "cli" && runsElsewhere(c, host);

  /** Continue here: where it's open already, else `claude --resume` in a
   * terminal pane on its host. `home`: its folder is gone, run it there.
   * `move`: it's running in another terminal, stop it there first (the
   * person said yes to Move here). */
  const goOn = async (c: Conversation, host: string, home = false, move = false) => {
    if (busy) return;
    // Running elsewhere: only Move here's button stops it.
    if (runsElsewhere(c, host) && !move) return;
    const there = host !== here && fleet;
    if (openHere(c, host)) {
      const p = c.block ?? c.live!.pane!;
      if (there) fleet.open(host, p);
      else client.focusPane(p);
      close();
      return;
    }
    setBusy(true);
    try {
      let v: ContinueConversationResponse | null;
      if (there) {
        const res = await within(fleet.request(host, OPS["conversation.continue"].method, opPath("conversation.continue"), { id: c.id, home } satisfies ContinueConversationRequest), HOST_MS);
        const body = await res.json<Partial<ContinueConversationResponse> & { error?: string }>().catch(() => null);
        v = res.ok && typeof body?.conversation === "string" ? (body as ContinueConversationResponse) : null;
        if (!v) client.toast(body?.error ?? `couldn't continue it on ${host} (${res.status})`);
      } else {
        if (move) setRefused(null);
        v = await client.continueConversation({
          id: c.id,
          home,
          move,
          from: where.split,
          session: where.session,
          refused: move ? (why) => setRefused({ id: c.id, host, why }) : undefined,
        });
      }
      // Moving refused: it's still running there; show it as it is.
      if (!v && move) load();
      if (!v) return;
      if (v.pane == null) {
        if (v.folder_gone) setGone({ id: c.id, host, cwd: v.cwd });
        return;
      }
      if (there) fleet.open(host, v.pane);
      conversationContinued();
      close();
    } catch (e) {
      client.toast(`couldn't continue it on ${host}: ${(e as Error).message}`);
    } finally {
      setBusy(false);
    }
  };

  /** As an agent block (`then` unset), or forked into one. */
  const asBlock = async (c: Conversation, host: string, then?: "fork") => {
    if (busy) return;
    if (host !== here && fleet) return asBlockThere(c, host, then);
    if (c.block !== null && !then) {
      client.focusPane(c.block);
      close();
      return;
    }
    setBusy(true);
    const block = await client.openConversation({ id: c.id, then, split: where.split, session: where.session });
    setBusy(false);
    if (block === null) return;
    conversationContinued();
    close();
  };

  // Another host's: opened there (its first session, a tab of its own),
  // then shown there, as the swarm opens a pane on its host.
  const asBlockThere = async (c: Conversation, host: string, then?: "fork") => {
    const f = fleet!;
    if (!then && c.block !== null) {
      f.open(host, c.block);
      close();
      return;
    }
    setBusy(true);
    try {
      const res = await within(f.request(host, OPS["conversation.open"].method, opPath("conversation.open"), { id: c.id, then: then ?? null } satisfies OpenConversationRequest), HOST_MS);
      const v = await res.json<Partial<OpenConversationResponse>>().catch(() => null);
      if (!res.ok || typeof v?.block !== "number") {
        client.toast(v?.error ?? `couldn't open it on ${host} (${res.status})`);
        return;
      }
      if (v.error) client.toast(v.error);
      f.open(host, v.block);
      conversationContinued();
      close();
    } catch (e) {
      client.toast(`couldn't open it on ${host}: ${(e as Error).message}`);
    } finally {
      setBusy(false);
    }
  };

  /** The row's one button. */
  const act = (r: Conv) => {
    const move = movable(r.c, r.host);
    if (gone?.id === r.c.id && gone.host === r.host) return void goOn(r.c, r.host, true, move);
    void goOn(r.c, r.host, false, move);
  };

  const menu = (e: MouseEvent, r: Conv) => {
    const { c, host } = r;
    const items: MenuItem[] = [];
    if (!c.live && c.block === null) items.push({ label: "Open as agent block", hint: "Stopped, showing its transcript", run: () => void asBlock(c, host) });
    items.push({ label: "Fork here", hint: "A new conversation from this point; the original stays as it was", attrs: { "data-fork-here": "" }, run: () => void asBlock(c, host, "fork") });
    openMenu(e, items, e.currentTarget as HTMLElement);
  };

  const onKey = (e: KeyboardEvent) => {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setSel((s) => Math.min(convs.length - 1, s + 1));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setSel((s) => Math.max(0, s - 1));
    } else if (e.key === "Enter" && !(e.target as Element).closest?.("button")) {
      e.preventDefault();
      const r = convs[sel];
      if (r) act(r);
    }
  };

  const title = dialog ? null : "Your work";
  const sub = `Claude Code and Codex conversations on ${multi ? "your machines" : here || "this machine"}, newest first.`;
  let i = -1;
  const list = (
    <div class="yw-list" ref={listRef} role="listbox" aria-label="Your work" tabIndex={dialog ? -1 : 0} onKeyDown={dialog ? undefined : onKey}>
      {!multi && !hosts?.[0]?.list.length && reading && !error && <EmptyLine role="status">Reading…</EmptyLine>}
      {!multi && hosts && !reading && !error && total === 0 && (
        <EmptyState
          where={dialog ? "pane" : "page"}
          title="No conversations yet."
          why={`When you use Claude Code or Codex on ${here || "this machine"}, each conversation shows here by its repo, ready to continue.`}
          action={
            !dialog && (
              <button class="primary" data-your-work-new disabled={!client.connected} onClick={() => client.intent({ op: "new_session", name: null, from_pane: null })}>
                New terminal
              </button>
            )
          }
          data-your-work-empty
        />
      )}
      {!multi && hosts && !reading && total > 0 && convs.length === 0 && (
        <EmptyLine clear={() => setQuery("")}>Nothing matches.</EmptyLine>
      )}
      {view.map(({ h, repos }) => (
        <div key={h.host} class="yw-host" data-host={h.host}>
          {multi && (
            <div class={`yw-host-head ${h.state}`} data-host-head={h.host}>
              <span class="yw-host-name">{h.host}</span>
              <span class="yw-host-state">{h.state === "reading" ? "reading…" : h.state === "ok" ? `${h.list.length} conversation${h.list.length === 1 ? "" : "s"}` : (h.note ?? "")}</span>
            </div>
          )}
          {repos.map((r) => {
            const rows = shown(r);
            const left = r.convs.length - rows.length;
            const branch = r.convs.find((c) => c.branch)?.branch;
            return (
              <section key={`${r.host}:${r.cwd}`} class="yw-repo" data-repo={r.cwd}>
                <div class="yw-repo-head" title={r.cwd}>
                  <b class="yw-repo-name">{repoName(r.cwd)}</b>
                  <span class="yw-mono">{tilde(r.cwd, r.home)}</span>
                  {branch && <span class="yw-mono">⎇ {branch}</span>}
                </div>
                <ul class="yw-rows">
                  {rows.map((c) => {
                    i += 1;
                    const n = i;
                    const row = { c, host: r.host };
                    const isSel = n === sel;
                    const isGone = gone?.id === c.id && gone.host === r.host;
                    const move = movable(c, r.host);
                    // Running where it can't be moved from (another host's,
                    // Codex, the desktop app): go on there, or fork it.
                    const stuck = runsElsewhere(c, r.host) && !move;
                    const no = refused?.id === c.id && refused.host === r.host ? refused : null;
                    const label = isGone ? "Run it in home" : openHere(c, r.host) ? "Go to pane" : move ? "Move here" : "Continue here";
                    const codex = c.agent === "codex";
                    return (
                      <li
                        key={`${r.host}:${c.id}`}
                        role="option"
                        aria-selected={isSel}
                        class={`yw-row${isSel ? " selected" : ""}`}
                        data-conversation={c.id}
                        data-host={r.host}
                        onClick={() => setSel(n)}
                        onDblClick={() => act(row)}
                      >
                        <span class="yw-first" title={c.first_prompt ?? c.title}>
                          {c.first_prompt ?? c.title}
                        </span>
                        {no ? (
                          <span class="yw-last ask" data-move-refused>
                            {no.why}
                          </span>
                        ) : move && isSel && !isGone ? (
                          <span class="yw-last ask" data-move-ask>
                            Running in {elsewhere(c)}. Move here?
                          </span>
                        ) : (
                          <span class="yw-last" data-folder-gone={isGone ? true : undefined}>
                            {isGone ? `${gone!.cwd} is gone. Run it in your home folder instead?` : (c.last_prompt ?? "")}
                          </span>
                        )}
                        <span class="yw-meta">
                          {(c.live || c.block !== null) && (
                            <span class="yw-live" data-live>
                              ● {openHere(c, r.host) ? "in a pane here" : `running in ${elsewhere(c)}`}
                            </span>
                          )}
                          <span class="yw-tool">{codex ? "Codex" : "Claude Code"}</span>
                          <Time class="yw-when" at={Math.min(c.updated_ms, Date.now())} />
                          <span class="yw-acts">
                            <button
                              class="primary small"
                              data-continue-here={label === "Continue here" ? true : undefined}
                              data-continue-home={isGone ? true : undefined}
                              data-go-to-pane={label === "Go to pane" ? true : undefined}
                              data-move-here={label === "Move here" ? true : undefined}
                              disabled={busy || stuck}
                              title={
                                stuck
                                  ? `It's running in ${elsewhere(c)}: go on there${codex ? "" : ", or fork it"}`
                                  : move
                                    ? "Stop it there (gently, as Ctrl-C would) and continue it here"
                                    : label === "Continue here"
                                      ? `Your own ${codex ? "codex resume" : "claude --resume"}, in a terminal pane in its folder`
                                      : undefined
                              }
                              onClick={(e) => {
                                e.stopPropagation();
                                setSel(n);
                                act(row);
                              }}
                            >
                              {label}
                            </button>
                            {!codex && (
                              <IconButton
                                label="More"
                                class="secondary sq small"
                                aria-haspopup="menu"
                                data-conv-more
                                disabled={busy}
                                onClick={(e: MouseEvent) => {
                                  e.stopPropagation();
                                  setSel(n);
                                  menu(e, row);
                                }}
                              >
                                <MoreIcon size="sm" />
                              </IconButton>
                            )}
                          </span>
                        </span>
                      </li>
                    );
                  })}
                </ul>
                {left > 0 && (
                  <button class="yw-more" data-repo-more onClick={() => setMore((m) => new Set(m).add(`${r.host}:${r.cwd}`))}>
                    {left} more in {repoName(r.cwd)}
                  </button>
                )}
              </section>
            );
          })}
        </div>
      ))}
      {error && <p class="error yw-error">{error}</p>}
    </div>
  );
  return (
    <div class={dialog ? "your-work in-dialog" : phone ? "your-work page phone" : "your-work page"} data-your-work onKeyDown={dialog ? onKey : undefined}>
      {dialog ? (
        <div class="yw-dialog-head">
          <h2>Your work</h2>
          <span class="yw-sub">{sub}</span>
          {/* A phone has no Escape: × closes it. */}
          <IconButton label="Close" title="Close" class="yw-close" data-your-work-close onClick={close}>
            <CloseIcon size="md" />
          </IconButton>
          <input
            ref={input}
            class="yw-filter"
            placeholder="Search prompts and folders"
            aria-label="Search prompts and folders"
            value={query}
            autocomplete="off"
            autocapitalize="off"
            spellcheck={false}
            onInput={(e) => {
              setQuery(e.currentTarget.value);
              setSel(0);
            }}
          />
        </div>
      ) : (
        <PageHeader title={title} sub={sub} />
      )}
      {list}
    </div>
  );
}
