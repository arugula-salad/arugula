// Claude Code conversations (M33): every one on the daemon's machine, from
// a terminal, the desktop app's Code tab or VS Code, grouped by folder, newest
// first, and Codex's with them, marked Codex. Picking one continues it
// here: a terminal pane in its folder running the person's own `claude
// --resume <id>` (or `codex resume <id>`), in a new tab named after it (no
// adapter to install), or the pane or block that has it already. If its
// folder is gone, the picker says so and offers to run it in home. One
// running in a terminal outside Arugula asks first ("Running in iTerm.
// Move here?"): *Move here* stops it there (SIGINT, then SIGTERM) and
// continues it here; if it won't stop, it says to quit it there. *Open
// as agent block* shows a Claude Code one as an agent block instead
// (stopped, its transcript as it grows), and *Fork here* forks it,
// leaving the original alone. Opened from a pane's menu, the agent dialog
// and the phone's sheet. On a phone it is a full-height sheet.
//
// With more than one host (#78), every host's are listed, grouped by host
// and then folder: each host is asked over the fleet's connection (M25) at
// once and shows up as it answers. One that's asleep isn't woken to be
// asked, and one that doesn't answer in a few seconds says so. Opening
// one of another host's opens it on that host and shows it there.
//
// A conversation whose Claude Code runs the arugula-live mod (#453) shows
// its live figures in its row, as a terminal's header does (#450): the
// slim strip, its popover saying "Live from Claude Code", and Claude
// Code's own cost over the estimate. From any pane or none: VS Code, the
// desktop app, the editor block.

import { OPS, opPath } from "../ops";
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "preact/hooks";
import type { ApiResponse, Client } from "../client";
import { directory } from "../hosts";
import type { ContinueConversationRequest, ContinueConversationResponse, OpenConversationRequest, OpenConversationResponse, PaneId } from "../../shared/proto";
import { getFleet } from "./hosts";
import { countView } from "../../shared/telemetry";
import { Time } from "../../shared/ui/time";
import { CLAUDE_COST_NOTE, COST_NOTE, ContextStrip, costLabel, usd, type AgentContext, type Estimate } from "../blocks/agent-context";
import { Chip } from "../../shared/ui/state-chip";
import { Modal } from "../../shared/ui/modal";
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
  /** What Claude Code said of it from inside, live (#453): the arugula-live
   *  mod's figures, from a pane or none. */
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

/** Show the conversations picker. */
export function pickConversation(client: Client, where: ConversationsWhere, phone = false) {
  countView("conversations");
  open = { client, where, phone };
  changed();
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
  return (
    <Picker
      client={client}
      where={where}
      phone={phone}
      close={() => {
        open = null;
        changed();
      }}
    />
  );
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

const SOURCE = { terminal: "Terminal", desktop: "Desktop", vscode: "VS Code", codex: "Codex", other: "Other" } as const;

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

function within<T>(p: Promise<T>, ms: number): Promise<T> {
  return new Promise((resolve, reject) => {
    const t = setTimeout(() => reject(new Error("not answering")), ms);
    p.then(
      (v) => (clearTimeout(t), resolve(v)),
      (e) => (clearTimeout(t), reject(e)),
    );
  });
}

type Row =
  | { kind: "host"; h: HostConvs }
  | { kind: "folder"; cwd: string; host: string; home: string | null }
  | { kind: "conv"; c: Conversation; host: string };

function Picker({ client, where, phone, close }: { client: Client; where: ConversationsWhere; phone: boolean; close: () => void }) {
  const [hosts, setHosts] = useState<HostConvs[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  const [all, setAll] = useState(false);
  const [liveOnly, setLiveOnly] = useState(false);
  const [sel, setSel] = useState(0);
  const [busy, setBusy] = useState(false);
  /** The conversation whose folder is gone, as Continue here found: offer home. */
  const [gone, setGone] = useState<{ id: string; host: string; cwd: string } | null>(null);
  const input = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLUListElement>(null);
  const loads = useRef(0);

  // This page's host, and with a fleet the others (#78).
  const fleet = getFleet();
  const here = directory.current;
  const multi = !!fleet && fleet.list.length > 1;

  const load = () => {
    const gen = ++loads.current;
    setError(null);
    const q = new URLSearchParams({ limit: "1000" });
    if (all) q.set("all", "1");
    if (liveOnly) q.set("live", "1");
    const path = `${opPath("conversations.list")}?${q}`;
    const others = multi ? fleet!.list.filter((h) => h.name !== here) : [];
    const start: HostConvs[] = [
      { host: here, state: "reading", list: [] },
      ...others.map((h): HostConvs => {
        if (h.state === "asleep") return { host: h.name, state: "asleep", list: [], note: "asleep" };
        if (h.state === "capped") return { host: h.name, state: "away", list: [], note: "not live (too many machines)" };
        return { host: h.name, state: "reading", list: [] };
      }),
    ];
    setHosts(start);
    const set = (h: HostConvs) => gen === loads.current && setHosts((cur) => cur && cur.map((x) => (x.host === h.host ? h : x)));
    for (const h of start) {
      if (h.state !== "reading") continue;
      const ask: Promise<ApiResponse> = h.host === here ? client.request(OPS["conversations.list"].method, path) : fleet!.request(h.host, OPS["conversations.list"].method, path);
      within(ask, HOST_MS)
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
  }, [all, liveOnly]);
  useLayoutEffect(() => {
    if (!phone) input.current?.focus();
  }, []);

  // Each host's folders by their newest conversation; this host first,
  // and in it the folder we came from.
  const rows: Row[] = useMemo(() => {
    if (!hosts) return [];
    const words = query.toLowerCase().split(/\s+/).filter(Boolean);
    const out: Row[] = [];
    for (const h of hosts) {
      if (multi) out.push({ kind: "host", h });
      const home = homeOf(h.list);
      const hits = h.list.filter((c) => {
        const hay = `${c.title} ${c.first_prompt ?? ""} ${c.last_prompt ?? ""} ${c.cwd}`.toLowerCase();
        return words.every((w) => hay.includes(w));
      });
      const groups = new Map<string, Conversation[]>();
      for (const c of hits) {
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
      for (const f of order) {
        out.push({ kind: "folder", cwd: f, host: h.host, home });
        for (const c of groups.get(f)!) out.push({ kind: "conv", c, host: h.host });
      }
    }
    return out;
  }, [hosts, query, where.cwd, here, multi]);
  const convs = rows.filter((r): r is Extract<Row, { kind: "conv" }> => r.kind === "conv");
  const reading = !!hosts && hosts.some((h) => h.state === "reading");

  useEffect(() => {
    listRef.current?.querySelector(".picker-row.selected")?.scrollIntoView({ block: "nearest" });
  }, [sel, rows]);

  /** The pane it runs in, if that's one of its host's panes. */
  const inPane = (c: Conversation, host: string): boolean => {
    const p = c.live?.pane;
    if (p === undefined) return false;
    if (host === here) return !!client.info(p);
    return !!fleet?.panes.some((x) => x.host === host && x.id === p && !x.stale);
  };

  /** Running at a terminal outside Arugula on this machine: Move here can
   * stop it there and go on with it here. Never another host's. */
  const movable = (c: Conversation, host: string): boolean =>
    host === here && c.agent !== "codex" && c.live?.entrypoint === "cli" && c.block === null && c.live.pane === undefined && c.live.block === undefined;

  /** Continue here: where it's open already, else `claude --resume` in a
   * terminal pane on its host. `home`: its folder is gone, run it there.
   * `move`: it's running in another terminal, stop it there first (the
   * person said yes to Move here). */
  const goOn = async (c: Conversation, host: string, home = false, move = false) => {
    if (busy) return;
    // Running elsewhere: only Move here's button stops it.
    if (c.live && c.block === null && !inPane(c, host) && !move) return;
    const there = host !== here && fleet;
    if (c.block !== null || inPane(c, host)) {
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
        v = await client.continueConversation({ id: c.id, home, move, from: where.split, session: where.session });
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

  const pick = async (c: Conversation, host: string, then?: "continue" | "fork") => {
    if (busy) return;
    if (host !== here && fleet) return pickThere(c, host, then);
    if (c.block !== null && !then) {
      client.focusPane(c.block);
      close();
      return;
    }
    if (c.live?.pane !== undefined && inPane(c, host) && !then) {
      // It's running in one of our panes: that's where it goes on.
      client.focusPane(c.live.pane);
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
  const pickThere = async (c: Conversation, host: string, then?: "continue" | "fork") => {
    const f = fleet!;
    if (!then && (c.block !== null || inPane(c, host))) {
      f.open(host, c.block ?? c.live!.pane!);
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

  const onKey = (e: KeyboardEvent) => {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setSel((s) => Math.min(convs.length - 1, s + 1));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setSel((s) => Math.max(0, s - 1));
    } else if (e.key === "Enter") {
      e.preventDefault();
      const r = convs[sel];
      if (r) void goOn(r.c, r.host, gone?.id === r.c.id && gone.host === r.host);
    }
  };

  const chosenRow = convs[sel];
  const chosen = chosenRow?.c;
  const chosenHost = chosenRow?.host ?? here;
  const chosenGone = !!chosen && gone?.id === chosen.id && gone.host === chosenHost ? gone : null;
  // Codex's has one way on, Continue here: no agent block, no fork.
  const codex = chosen?.agent === "codex";
  const move = !!chosen && movable(chosen, chosenHost);
  let i = -1;
  return (
    <Modal bare close={close} label="Claude Code conversations" class={phone ? "picker conversations phone" : "picker conversations"} root={{ onKeyDown: onKey }}>
      <div class="picker-where">
        <span class="picker-host">Claude Code conversations</span>
        <label class="check">
          <input type="checkbox" role="switch" class="sw-switch" checked={liveOnly} onChange={(e) => setLiveOnly(e.currentTarget.checked)} />
          Open now
        </label>
        <label class="check" title="claude -p and SDK runs, archived ones, ones whose folder is gone">
          <input type="checkbox" role="switch" class="sw-switch" checked={all} onChange={(e) => setAll(e.currentTarget.checked)} />
          All
        </label>
      </div>
      <input
        ref={input}
        class="picker-filter"
        placeholder="Search titles, prompts and folders"
        value={query}
        autocomplete="off"
        autocapitalize="off"
        spellcheck={false}
        onInput={(e) => {
          setQuery(e.currentTarget.value);
          setSel(0);
        }}
      />
      <ul class="picker-list" role="listbox" ref={listRef}>
        {!multi && reading && !error && <li class="picker-empty">Reading…</li>}
        {!multi && hosts && !reading && convs.length === 0 && <li class="picker-empty">{query ? "Nothing matches" : "No conversations here"}</li>}
        {rows.map((r) => {
          if (r.kind === "host") {
            const h = r.h;
            const n = h.list.length;
            const what = h.state === "reading" ? "reading…" : h.state === "ok" ? `${n} conversation${n === 1 ? "" : "s"}` : (h.note ?? "");
            return (
              <li key={`h:${h.host}`} class={`conv-host ${h.state}`} data-host={h.host}>
                <span class="conv-host-name">{h.host}</span>
                <span class="conv-host-state">{what}</span>
              </li>
            );
          }
          if (r.kind === "folder") {
            return (
              <li key={`f:${r.host}:${r.cwd}`} class="conv-folder" title={r.cwd}>
                {tilde(r.cwd, r.home) || "(no folder)"}
              </li>
            );
          }
          const { c, host } = r;
          i += 1;
          const n = i;
          return (
            <li
              key={`${host}:${c.id}`}
              role="option"
              aria-selected={n === sel}
              class={`picker-row conv-row${n === sel ? " selected" : ""}`}
              data-conversation={c.id}
              data-host={host}
              onClick={() => {
                setSel(n);
                void goOn(c, host);
              }}
              title={c.first_prompt ?? c.title}
            >
              <span class={`conv-source ${c.source}`}>{SOURCE[c.source]}</span>
              <span class="picker-label conv-title">{c.title}</span>
              {c.live && <span class="conv-live" title={c.live.place}>● {c.live.pane !== undefined ? `%${c.live.pane}` : "open"}</span>}
              {c.block !== null && <Chip kind="tag" tone="info">%{c.block}</Chip>}
              {c.context && (
                // Its popover, not the row: a click on the strip opens that.
                <span class="conv-context" data-conv-context onClick={(e) => e.stopPropagation()}>
                  <ContextStrip c={c.context} slim />
                </span>
              )}
              {c.context?.cost_usd != null ? (
                <span class="conv-cost" data-cost={c.context.cost_usd} data-cost-from="claude" title={CLAUDE_COST_NOTE}>
                  {usd(c.context.cost_usd)}
                </span>
              ) : (
                c.estimate && (
                  <span class="conv-cost" data-cost={c.estimate.usd ?? ""} title={`${COST_NOTE}${c.estimate.subagents.length ? ` With its subagents.` : ""}`}>
                    {costLabel(c.estimate)}
                  </span>
                )
              )}
              <Time class="conv-when" at={Math.min(c.updated_ms, Date.now())} />
            </li>
          );
        })}
      </ul>
      {error && <p class="error picker-error">{error}</p>}
      <div class="picker-actions">
        <span
          class={move ? "picker-target ask" : "picker-target"}
          title={chosenGone?.cwd ?? chosen?.live?.place ?? chosen?.cwd}
          data-folder-gone={chosenGone ? true : undefined}
          data-move-ask={move ? true : undefined}
        >
          {chosenGone
            ? `${chosenGone.cwd} is gone. Run it in your home folder instead?`
            : move
              ? `Running in ${chosen!.live!.terminal ?? "another terminal"}. Move here?`
              : chosen
              ? chosen.live
                ? chosen.live.place
                : (chosen.last_prompt ?? chosen.first_prompt ?? "")
              : ""}
        </span>
        {chosenGone ? (
          <button class="primary" data-continue-home disabled={busy} onClick={() => chosen && void goOn(chosen, chosenHost, true, !!chosen.live)}>
            Run it in home
          </button>
        ) : move ? (
          <button class="primary" data-move-here disabled={busy} title="Stop it there (gently, as Ctrl-C would) and continue it here" onClick={() => chosen && void goOn(chosen, chosenHost, false, true)}>
            Move here
          </button>
        ) : (
          <button
            class="primary"
            data-continue-here
            disabled={!chosen || busy || (!!chosen.live && chosen.block === null && !inPane(chosen, chosenHost))}
            title={
              chosen?.live && chosen.block === null && !inPane(chosen, chosenHost)
                ? `It's ${chosen.live.place}: ${codex ? "go on there" : "open it as an agent block and fork it"}`
                : `Your own ${codex ? "codex resume" : "claude --resume"}, in a terminal pane in its folder`
            }
            onClick={() => chosen && void goOn(chosen, chosenHost)}
          >
            {chosen?.block != null ? "Go to it" : chosen && inPane(chosen, chosenHost) ? "Go to pane" : "Continue here"}
          </button>
        )}
        {!codex && (
          <button disabled={!chosen || busy} data-open-block title="Stopped, showing its transcript" onClick={() => chosen && void pick(chosen, chosenHost)}>
            {chosen?.block != null ? "Go to its agent block" : "Open as agent block"}
          </button>
        )}
        {!codex && (
          <button disabled={!chosen || busy} data-fork-here title="A new session with its history; the original is left alone" onClick={() => chosen && void pick(chosen, chosenHost, "fork")}>
            Fork here
          </button>
        )}
        <button onClick={close}>Cancel</button>
      </div>
    </Modal>
  );
}
