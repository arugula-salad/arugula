// The attention drawer (M111, #1026): what waits on you, on every machine,
// from the fleet model the swarm already builds. The
// header's bell, the footer's "needs you" and the sidebar's Needs you
// open it, and count the same (shared/needs-you, UI audit #154). Selecting an item shows what it would run, where and who asks;
// Allow and Deny (or the question's answer) go through the existing
// `act`, and Go to pane only goes there. Looking at the drawer, or closing
// it, never answers or dismisses anything. Viewers see the items without
// buttons. Empty, it says what lands here and offers what would bring it:
// Claude Code's hooks where they're missing, and this device's
// notifications where they're off (Discovery 5).

import { useEffect, useLayoutEffect, useRef, useState } from "preact/hooks";
import { type Client } from "../../client";
import type { FleetPane } from "../../fleet";
import { directory } from "../../hosts";
import { OPS, opPath } from "../../ops";
import { gateKey, type ActRequest, type PaneInfo } from "../../../shared/proto";
import { needsYou, NEEDS_YOU } from "../../../shared/needs-you";
import { agentDisplay, cardTitle, kindOf, reasonOf, shortHost, whatItDoes } from "../../swarm/model";
import { AskCard, type Ask } from "../../../shared/ask";
import { PermissionBody, PermissionButtons, permissionTarget, VIEWER_NOTE } from "../answer-card";
import { DiffCard } from "../diff-card";
import { editDiff } from "../term-ask";
import { useSubscribe } from "../../../shared/ui/hooks";
import { getFleet } from "../hosts";
import { openMenu, type MenuItem } from "../../../shared/ui/menu";
import { agentNotifyItems, notificationItems, pushNow, subscribePush, toggleNotify } from "../notify";
import { hooksInstalled, setUpClaude } from "../agents-setup";
import { addPane } from "./add-pane";
import { closePlace, currentPlace } from "../places";

// ---- the items

export interface AttentionItem {
  p: FleetPane;
  /** On the machine this page shows (its own client acts on it). */
  here: boolean;
  /** It asks something of you now; else it failed. Both need you. */
  waiting: boolean;
  /** You may only watch it. */
  viewer: boolean;
  /** Its tab's name, when it's on the machine shown. */
  tab?: string;
}

// A failure waits too, until someone looks or dismisses it; a plain
// "finished" doesn't (the swarm's rail and Chat's Activity have those).
const wants = needsYou;

/** Owner-only asks (#234 invites, M79 tasks from another agent). */
const ownersOnly = (info: PaneInfo) => info.ask?.source === "invite" || info.ask?.source === "a2a";

/** Everything that wants you: the shown machine's panes from its own
 * client (live, and it's the one that can act), every other machine's from
 * the fleet's summaries. Waiting first, then newest first. */
export function attentionItems(client: Client): AttentionItem[] {
  const out: AttentionItem[] = [];
  const current = directory.current;
  const st = client.state;
  if (st) {
    for (const info of st.panes) {
      // A pane from another machine in this layout: its own machine lists it.
      if (!wants(info)) continue;
      const tab = client.tabOfPane(info.id);
      const sid = tab ? (client.sessionOfTab(tab.id) ?? null) : null;
      const s = st.sessions.find((x) => x.id === sid);
      const viewer = ownersOnly(info) ? !!st.roles : client.role(sid) === "viewer";
      out.push({
        p: {
          key: `${current}:${info.id}`,
          host: current,
          id: info.id,
          info,
          session: s ? { id: s.id, name: s.name } : null,
          stale: !client.connected,
          person: { id: "me", name: "me", kind: "me" },
          driver: info.driver ?? null,
          watchers: [],
          unread: 0,
          mention: false,
        },
        here: true,
        waiting: info.attention === "needs_input",
        viewer,
        tab: tab ? (tab.name ?? client.title(info.id)) : undefined,
      });
    }
  }
  const fleet = getFleet();
  const others = fleet ? fleet.panes.filter((p) => p.host !== current) : [];
  for (const p of others) {
    if (!wants(p.info)) continue;
    const roles = fleet?.host(p.host)?.summary?.roles;
    const viewer = ownersOnly(p.info) ? !!roles : fleet?.role(p) === "viewer";
    out.push({ p, here: false, waiting: p.info.attention === "needs_input", viewer });
  }
  const since = (i: AttentionItem) => i.p.info.reason?.since_ms ?? i.p.info.ask?.at_ms ?? 0;
  return out.sort((a, b) => Number(b.waiting) - Number(a.waiting) || since(b) - since(a));
}

/** How many need you, on every machine: the bell's and the footer's. */
export function waitingCount(client: Client): number {
  return attentionItems(client).length;
}

/** Re-render on the shown machine's state and the fleet's. */
export function useAttentionTick(client: Client) {
  useSubscribe((fn) => client.subscribe(fn));
  useSubscribe((fn) => getFleet()?.subscribe(fn) ?? (() => {}));
  useSubscribe((fn) => directory.subscribe(fn));
}

// ---- open and closed

let drawer: { back: HTMLElement | null; pick?: string } | null = null;
const listeners = new Set<() => void>();
const changed = () => listeners.forEach((fn) => fn());

/** Open it; Escape or Close gives the focus back to what had it. `pick`
 * selects that item (`host:pane`) first. */
export function openAttention(pick?: string) {
  addPane.close(false);
  const back = document.activeElement instanceof HTMLElement ? document.activeElement : null;
  drawer = { back: drawer?.back ?? back, pick };
  changed();
}

export function closeAttention(restore = true) {
  const back = drawer?.back;
  drawer = null;
  changed();
  if (restore && back?.isConnected) back.focus();
}

export function toggleAttention() {
  if (drawer) closeAttention();
  else openAttention();
}

export function useAttentionOpen(): boolean {
  const [, tick] = useState(0);
  useEffect(() => {
    const fn = () => tick((t) => t + 1);
    listeners.add(fn);
    return () => void listeners.delete(fn);
  }, []);
  return drawer !== null;
}

// ---- the bell

const BellIcon = () => (
  <svg class="icon" width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
    <path d="M6 16.5V11a6 6 0 0 1 12 0v5.5l1.5 2h-15z" />
    <path d="M10 20.5a2 2 0 0 0 4 0" />
  </svg>
);

/** The header's bell: how many wait on you, on every machine. */
export function AttentionBell({ client }: { client: Client }) {
  useAttentionTick(client);
  const open = useAttentionOpen();
  const n = waitingCount(client);
  const label = n ? `${NEEDS_YOU}: ${n}` : `${NEEDS_YOU}: nothing`;
  return (
    <button
      class={`ws-bell${n ? " has" : ""}${open ? " on" : ""}`}
      data-open-attention
      data-bell-count={n}
      aria-label={label}
      aria-expanded={open}
      aria-haspopup="dialog"
      title={`${label}, on every machine (looking doesn't answer anything)`}
      onClick={toggleAttention}
    >
      <BellIcon />
      {n > 0 && (
        <span class="ws-bell-count" aria-hidden="true">
          {n > 99 ? "99+" : n}
        </span>
      )}
    </button>
  );
}

// ---- the drawer

/** Who asks, in words: "Claude Code", "hud". */
function agentOf(p: FleetPane): string | null {
  const a = p.info.reason?.ask?.agent ?? p.info.ask?.agent ?? (p.info.ask?.source === "hook" ? "claude" : null);
  if (a) return agentDisplay(a);
  return kindOf(p) === "agent" ? whatItDoes(p) : null;
}

/** What it would run or did: the permission's command or file, else the
 * failed command, else the pane's. */
function commandOf(p: FleetPane): string | null {
  const ask = p.info.ask;
  if (ask?.kind === "permission") return permissionTarget(ask);
  return p.info.reason?.command ?? p.info.current?.text ?? p.info.command ?? null;
}

function titleOf(i: AttentionItem): string {
  const r = reasonOf(i.p);
  if (!r) return "Needs you";
  return cardTitle(r, 1, [shortHost(i.p.host)], agentOf(i.p) ?? undefined);
}

/** Go to the item's pane, on its machine, without answering. */
export function goToItem(client: Client, i: AttentionItem) {
  closeAttention(false);
  const at = currentPlace();
  if (at !== "panes") closePlace(at);
  if (i.here) {
    client.setActive(i.p.id);
    requestAnimationFrame(() => client.viewOf(i.p.id)?.focus());
  } else getFleet()?.open(i.p.host, i.p.id);
}

export function AttentionDrawer({ client }: { client: Client }) {
  const open = useAttentionOpen();
  useAttentionTick(client);
  const [sel, setSel] = useState<string | null>(null);
  const list = useRef<HTMLDivElement>(null);
  // A pick from where it was opened, once.
  useLayoutEffect(() => {
    if (!open) return;
    if (drawer?.pick) {
      setSel(drawer.pick);
      drawer.pick = undefined;
    }
    (list.current?.querySelector(".ws-att-item.selected") as HTMLElement | null)?.focus() ?? list.current?.focus();
  }, [open]);
  if (!open) return null;
  const items = attentionItems(client);
  const chosen = items.find((i) => i.p.key === sel) ?? items[0];
  const waiting = items.filter((i) => i.waiting).length;
  const onKey = (e: KeyboardEvent) => {
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      closeAttention();
    } else if ((e.key === "ArrowDown" || e.key === "ArrowUp") && (e.target as HTMLElement).closest(".ws-att-list")) {
      e.preventDefault();
      if (!items.length) return;
      const at = Math.max(0, items.findIndex((i) => i === chosen));
      const next = items[(at + (e.key === "ArrowDown" ? 1 : items.length - 1)) % items.length];
      setSel(next.p.key);
      requestAnimationFrame(() => (list.current?.querySelector(`[data-att-key="${CSS.escape(next.p.key)}"]`) as HTMLElement | null)?.focus());
    }
  };
  const prefs = (e: MouseEvent) => {
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    const items: MenuItem[] = [
      { header: "Notifications" },
      ...notificationItems(client),
      ...(client.session !== null ? agentNotifyItems(client, client.session) : []),
    ];
    openMenu({ clientX: r.left, clientY: r.top - 4, preventDefault: () => e.preventDefault() }, items);
  };
  return (
    <div class="ws-att" role="dialog" aria-label="Needs you" data-attention-drawer onKeyDown={onKey}>
      <header class="ws-att-head">
        <h2>Needs you</h2>
        <span class="ws-att-count">
          {waiting ? `${waiting} waiting` : "nothing waiting"}
          {items.length > waiting ? `, ${items.length - waiting} failed` : ""}
        </span>
        <button class="ws-icon-button" aria-label="Close" title="Close (Escape); nothing is answered" onClick={() => closeAttention()} data-close-attention>
          ✕
        </button>
      </header>
      <div class="ws-att-list" role="listbox" aria-label="What needs you" ref={list} tabIndex={-1}>
        {items.length === 0 && (
          <Empty client={client} />
        )}
        {items.map((i) => (
          <button
            key={i.p.key}
            role="option"
            aria-selected={i === chosen}
            class={`ws-att-item${i === chosen ? " selected" : ""}`}
            data-att-key={i.p.key}
            data-att-pane={i.p.id}
            tabIndex={i === chosen ? 0 : -1}
            onClick={() => setSel(i.p.key)}
          >
            <span class={`ws-dot ${i.waiting ? "needs_input" : "failed"}`} aria-hidden="true" />
            {/* What it's about as the title, why and where under it (UI
                audit #172): every title was "Needs you" before. */}
            <span class="ws-att-item-text">
              <span class="ws-att-item-title">{whatItDoes(i.p)}</span>
              <span class="ws-att-item-sub">
                {[titleOf(i), shortHost(i.p.host), i.p.session?.name].filter(Boolean).join(" · ")}
              </span>
            </span>
          </button>
        ))}
      </div>
      {chosen && <Detail key={chosen.p.key} client={client} item={chosen} />}
      <footer class="ws-att-foot">
        <span>Looking here answers nothing.</span>
        <button class="link" data-attention-prefs onClick={prefs}>
          Notifications…
        </button>
      </footer>
    </div>
  );
}

/** Nothing waits: what would land here, and the steps that make it land.
 * Set up only on the owner's own daemon while Claude Code's settings lack
 * Arugula's hooks; Notify this device only while it could be on. */
function Empty({ client }: { client: Client }) {
  useSubscribe(subscribePush);
  const mine = !client.e2e && !client.state?.roles;
  const [hooks, setHooks] = useState<"missing" | "busy" | { ok: boolean; said: string } | null>(null);
  useEffect(() => {
    if (!mine) return;
    let live = true;
    void hooksInstalled().then((has) => live && !has && setHooks("missing"));
    return () => void (live = false);
  }, [mine]);
  const setUp = async () => {
    setHooks("busy");
    setHooks(await setUpClaude());
  };
  const push = pushNow() === "off";
  return (
    <div class="ws-empty ws-att-empty" role="status" data-attention-empty>
      <p>Nothing needs you on any machine.</p>
      <p class="dim">Claude in a terminal? Its questions land here, to answer from any device.</p>
      {typeof hooks === "object" && hooks !== null && (
        <p class={hooks.ok ? "dim" : "control-error"} data-attention-setup-said={hooks.ok ? "done" : "failed"}>
          {hooks.ok ? `${hooks.said} A Claude started from now on asks here.` : hooks.said}
        </p>
      )}
      {(hooks === "missing" || hooks === "busy" || push) && (
        <div class="ws-att-acts">
          {(hooks === "missing" || hooks === "busy") && (
            <button class="primary" disabled={hooks === "busy"} onClick={() => void setUp()} data-attention-setup title="Install Arugula's hooks in Claude Code's settings">
              {hooks === "busy" ? "Setting up…" : "Set up"}
            </button>
          )}
          {push && (
            <button class="secondary" onClick={() => void toggleNotify(client)} data-attention-notify title="Notifications on this device when something needs you">
              Notify this device
            </button>
          )}
        </div>
      )}
    </div>
  );
}

function Detail({ client, item }: { client: Client; item: AttentionItem }) {
  const [busy, setBusy] = useState(false);
  const p = item.p;
  const r = reasonOf(p);
  const ask: Ask | null = p.info.ask ?? null;
  const command = commandOf(p);
  const agent = agentOf(p);
  const act = async (body: ActRequest) => {
    setBusy(true);
    try {
      if (item.here) await client.act(body);
      else {
        const res = await getFleet()!.request(p.host, "POST", "/api/attention/act", body);
        if (!res.ok) client.toast((await res.json<{ error?: string }>().catch(() => null))?.error ?? `couldn't (${res.status})`);
      }
    } catch (e) {
      client.toast(String(e));
    } finally {
      setBusy(false);
    }
  };
  const request = (method: string, path: string) => (item.here ? client.request(method, path) : getFleet()!.request(p.host, method, path));
  const run = (action: ActRequest["action"], extra: Partial<ActRequest> = {}) =>
    void act({ action, pane: p.id, id: r?.ask?.id ?? (r?.gate ? gateKey(r.gate) : ask?.id), ...extra });
  const rows: [string, string | null | undefined][] = [
    ["Command", command],
    ["Machine", p.host],
    ["Directory", p.info.cwd],
    ["Agent", agent],
    ["Where", [p.session?.name, item.tab].filter(Boolean).join(" › ") || null],
  ];
  const diff = r?.kind === "diff" ? (p.info.diff ?? r.diff ?? null) : null;
  const go = (
    <button class="secondary" data-attention-goto onClick={() => goToItem(client, item)} title="Show it, on its machine; this doesn't answer it">
      Go to pane
    </button>
  );
  // Go to pane ends the item's own row of buttons, whatever kind it is,
  // or is the row when it has none (UI audit #172, #194).
  return (
    <section class="ws-att-detail" aria-label={titleOf(item)} data-attention-detail={p.key}>
      <h3>{titleOf(item)}</h3>
      {r && r.kind !== "ask" && <p class="ws-att-headline">{r.headline}</p>}
      <dl class="ws-att-facts">
        {rows.map(([k, v]) =>
          v ? (
            <div key={k} data-fact={k.toLowerCase()}>
              <dt>{k}</dt>
              <dd class={k === "Command" || k === "Directory" ? "mono" : undefined} title={v}>
                {v}
              </dd>
            </div>
          ) : null,
        )}
      </dl>
      <div class="ws-att-answer">
        {item.viewer ? (
          <>
            <p class="ask-viewer">{VIEWER_NOTE}</p>
            <div class="ws-att-acts">{go}</div>
          </>
        ) : p.stale && !item.here ? (
          <>
            <p class="ask-viewer">{shortHost(p.host)} isn't connected: go to the pane to answer once it is.</p>
            <div class="ws-att-acts">{go}</div>
          </>
        ) : diff ? (
          <DiffCard
            diff={diff}
            can
            act={(action, extra) => act({ action, pane: p.id, id: diff.id, ...extra })}
            full={async () => {
              const res = await request(OPS["pane.diff"].method, opPath("pane.diff", { id: p.id }));
              return res.ok ? (await res.json<{ new: string }>()).new : null;
            }}
            edit={item.here && !client.state?.roles ? () => void editDiff(client, p.id, diff) : undefined}
            more={go}
          />
        ) : ask?.kind === "permission" ? (
          <div class="ask perm" data-ask={ask.id}>
            <PermissionBody ask={ask} />
            <PermissionButtons ask={ask} act={(action, extra) => run(action, { ...extra, id: ask.id })} more={go} />
          </div>
        ) : ask && r?.ask?.what === "question" ? (
          <AskCard ask={ask} actions={{ answer: (content) => run("answer", { content, id: ask.id }), decline: () => run("deny", { id: ask.id }) }} more={go} />
        ) : r ? (
          <div class="ws-att-acts">
            {r.actions.includes("allow") && (
              <button class="primary" disabled={busy} onClick={() => run("allow")}>
                {r.kind === "gate" ? "Approve" : "Allow"}
              </button>
            )}
            {r.actions.includes("rerun") && (
              <button class="primary" disabled={busy} onClick={() => run("rerun")}>
                Rerun
              </button>
            )}
            {r.actions.includes("continue") && (
              <button class="primary" disabled={busy} onClick={() => run("continue")}>
                Continue
              </button>
            )}
            {r.actions.includes("deny") && (
              <button class="secondary danger" disabled={busy} onClick={() => run("deny")}>
                Deny
              </button>
            )}
            {r.actions.includes("dismiss") && (
              <button class="secondary" disabled={busy} onClick={() => run("dismiss")}>
                Dismiss
              </button>
            )}
            {go}
          </div>
        ) : (
          <div class="ws-att-acts">{go}</div>
        )}
      </div>
    </section>
  );
}
