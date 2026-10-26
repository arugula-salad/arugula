// Phone layout: one pane at a time, full screen. A sheet lists sessions,
// tabs and panes to switch between, and a key bar supplies the keys a phone
// keyboard lacks.

import { openAgentsPage } from "./agents-route";
import { openDocsPage } from "./docs-route";
import { useState } from "preact/hooks";
import { paneIds, tabLabel, type Client } from "../client";
import { EXPIRE_TITLE, gateKey } from "../../shared/proto";
import { useSubscribe } from "../../shared/ui/hooks";
import { AttentionBadge } from "./attention";
import { HostCrumb, HostSection } from "./hosts";
import { openSwarm } from "../swarm/route";
import { hasChat, openChat } from "./places";
import { ChatTabBadge, openChanges, openIssue, openPort, openPr } from "../blocks";
import { startAgent } from "./agent-dialog";
import { pickConversation } from "./conversations";
import { pickApp, openSandboxes } from "../labs-load";
import { openPicker } from "./picker";
import { NotifySection } from "./notify";
import { openDeveloper } from "./developer";
import { openMachineTelemetry } from "./machine-telemetry";
import { openGettingStarted } from "./welcome";
import { openPalette } from "./palette";
import { GateWhy } from "./gate-why";
import { PointChoices, pointOf } from "./point-choices";
import { shareSession } from "./people";
import { accountHeader, useAccount } from "./app-bar";
import { docShareItems } from "./doc-share";
import { needsYou } from "../../shared/needs-you";

export function PhoneHeader({ client }: { client: Client }) {
  const [open, setOpen] = useState(false);
  const tab = client.tabView();
  const session = client.state?.sessions.find((s) => s.id === client.session);
  const panes = tab ? paneIds(tab) : [];
  const active = client.active();
  return (
    <>
      <header class="bar phone-bar">
        <button class="sheet-button" aria-expanded={open} onClick={() => setOpen(!open)}>
          {client.state?.panes.some(needsYou) ? <span class="att needs_input">●</span> : "☰"} <HostCrumb />
          {session && (
            <>
              <span class="crumb">{session.name}</span> ›{" "}
              {tab && client.tabMachine(tab.id) && <span class="host-tag">VM</span>}
              <span class="crumb">{tab ? tabLabel(client, tab) : ""}</span>
            </>
          )}
        </button>
        {panes.length > 1 && (
          <span class="pane-count">
            {panes.indexOf(active ?? -1) + 1}/{panes.length}
          </span>
        )}
      </header>
      {open && <Sheet client={client} close={() => setOpen(false)} />}
    </>
  );
}

const TABS = [
  ["here", "Here"],
  ["machines", "Machines"],
  ["more", "More"],
] as const;

function Sheet({ client, close }: { client: Client; close: () => void }) {
  const state = client.state!;
  // The sheet is mounted fresh each time it opens, so it always lands on Here.
  const [tab, setTab] = useState<(typeof TABS)[number][0]>("here");
  const active = client.active();
  // Other sessions fold to a line each, so the one in use (and what's
  // below it) isn't pushed off the screen; a tap opens one.
  const [unfolded, setUnfolded] = useState<Set<number>>(() => new Set(client.session === null ? [] : [client.session]));
  const fold = (id: number) => {
    const next = new Set(unfolded);
    if (!next.delete(id)) next.add(id);
    setUnfolded(next);
  };
  // Gates first (M34): a release waiting for a person is the most likely
  // reason to have opened this on a phone.
  // What needs you, as the bell counts it: not what only finished (UI
  // audit #154).
  const wanting = state.panes
    .filter(needsYou)
    .sort((a, b) => Number(b.reason?.kind === "gate") - Number(a.reason?.kind === "gate"));
  const act = (fn: () => void) => () => {
    fn();
    close();
  };
  const session = client.session;
  // #331: with no session, what opens a tab opens a session for it.
  const where = session ?? undefined;
  const owner = !state.roles;
  const closeTab =
    client.tab !== null && (client.tabMachine(client.tab) || paneIds(client.tabView(client.tab)!).length > 1);
  return (
    <div class="sheet-backdrop" onClick={close}>
      <nav class="sheet" onClick={(e) => e.stopPropagation()}>
        <div class="sheet-panel" role="tabpanel" id={`sheet-panel-${tab}`} aria-labelledby={`sheet-tab-${tab}`} data-sheet-panel={tab}>
          {tab === "here" && (
            <>
            {wanting.length > 0 && (
              <section class="sheet-card needs-you">
                <h2>Needs you</h2>
                {wanting.map((p) => {
                  const may = client.role(client.sessionOfTab(client.tabOfPane(p.id)?.id ?? -1) ?? null) !== "viewer";
                  const rerun = p.reason?.actions.includes("rerun") && may;
                  const gate = p.reason?.kind === "gate" ? p.reason.gate : undefined;
                  // #621: a decision point's question is answered, not approved.
                  const point = pointOf(gate);
                  return (
                    <div key={p.id} class="sheet-row" data-wants={p.id}>
                      <button class="sheet-item" onClick={act(() => client.setActive(p.id))}>
                        <AttentionBadge state={p.attention} reason={p.reason} />{" "}
                        <span class="sheet-wants">
                          {p.reason?.headline || client.title(p.id) || p.current?.text || p.last?.text || p.cwd || `pane %${p.id}`}
                          {gate && <GateWhy gate={gate} compact />}
                        </span>
                      </button>
                      {rerun && (
                        <button class="sheet-act" data-rerun={p.id} onClick={act(() => void client.act({ action: "rerun", pane: p.id }))}>
                          Rerun
                        </button>
                      )}
                      {gate && point && may && (
                        <PointChoices
                          point={point}
                          cls="sheet-act"
                          answer={(answer) => act(() => void client.act({ action: "answer", pane: p.id, id: gateKey(gate), content: { answer } }))()}
                        />
                      )}
                      {gate && !point && may && (
                        <button class="sheet-act" data-approve-gate={p.id} onClick={act(() => void client.act({ action: "allow", pane: p.id, id: gateKey(gate) }))}>
                          Approve
                        </button>
                      )}
                      {gate && may && p.reason?.actions.includes("expire") && (
                        <button
                          class="sheet-act"
                          data-expire-gate={p.id}
                          title={EXPIRE_TITLE}
                          onClick={act(() => void client.act({ action: "expire", pane: p.id, id: gateKey(gate) }))}
                        >
                          Expire
                        </button>
                      )}
                    </div>
                  );
                })}
              </section>
            )}
              {/* What a phone is opened for most: right at the top. */}
              <div class="sheet-actions sheet-quick">
                <button
                  data-new-tab
                  onClick={act(() =>
                    client.intent(session !== null ? { op: "new_tab", session, from_pane: active ?? null } : { op: "new_session", name: null, from_pane: null }),
                  )}
                >
                  New tab
                </button>
                <button data-new-agent onClick={act(() => startAgent(client, { session: where, from: active }))}>
                  New agent
                </button>
                {hasChat() ? (
                  <button data-open-chat onClick={act(() => openChat())}>
                    Chat
                  </button>
                ) : (
                  <button data-new-chat onClick={act(() => void client.newChat({ session: where ?? undefined }))}>
                    New chat
                  </button>
                )}
                {owner && (
                  <button
                    data-conversations
                    onClick={act(() => pickConversation(client, { session: where, cwd: (active !== undefined && client.cwd(active)) || undefined }, true))}
                  >
                    Conversations
                  </button>
                )}
              </div>
            <section class="sheet-sessions">
              <h2>Sessions</h2>
              {state.sessions.map((s) => {
                const open = unfolded.has(s.id);
                const here = s.id === session;
                return (
                  <div key={s.id} class={here ? "sheet-card sheet-session here" : "sheet-card sheet-session"}>
                    <button class="sheet-session-head" aria-expanded={open} data-session={s.id} onClick={() => fold(s.id)}>
                      <span class="fold">{open ? "▾" : "▸"}</span>
                      <span class="name">{s.name}</span>
                      <span class="count">
                        {s.tabs.length} tab{s.tabs.length === 1 ? "" : "s"}
                      </span>
                    </button>
                    {open &&
                      s.tabs.map((tid) => {
                        const t = client.tabView(tid);
                        if (!t) return null;
                        const panes = paneIds(t);
                        return (
                          <div key={tid} class="sheet-tab">
                            <button class={tid === client.tab ? "sheet-item current" : "sheet-item"} onClick={act(() => client.selectTab(tid))}>
                              {(client.tabMachine(tid) || panes.some((p) => client.machine(p))) && <span class="host-tag">VM</span>}
                              <span class="label">{tabLabel(client, t)}</span>
                              {panes.length > 1 && <span class="count">{panes.length}</span>}
                              <ChatTabBadge client={client} panes={panes} />
                            </button>
                            {panes.length > 1 &&
                              panes.map((p, i) => (
                                <button
                                  key={p}
                                  class={p === active ? "sheet-item sheet-pane current" : "sheet-item sheet-pane"}
                                  onClick={act(() => client.setActive(p))}
                                >
                                  {/* Shells often title every pane alike; the number tells them apart. */}
                                  <span class="pane-number">{i + 1}</span>
                                  <span class="label">{client.title(p) || client.cwd(p) || `pane %${p}`}</span>
                                </button>
                              ))}
                          </div>
                        );
                      })}
                  </div>
                );
              })}
            </section>
            </>
          )}
          {tab === "machines" && <HostSection close={close} />}
          {tab === "more" && (
            <>
              {(active !== undefined || closeTab || (owner && session !== null)) && (
                <section class="sheet-group">
                  <h2>This pane</h2>
                  <div class="sheet-actions">
                    {active !== undefined && <button onClick={act(() => openPicker(client, active, true))}>Go to directory</button>}
                    {active !== undefined && (
                      <button onClick={act(() => client.intent({ op: "split", pane: active, edge: "right" }))}>Split pane</button>
                    )}
                    {active !== undefined && client.tab !== null && client.tabMachine(client.tab) && (
                      <button onClick={act(() => client.intent({ op: "split", pane: active, edge: "right", local: true }))}>Split (local)</button>
                    )}
                    {/* #237: the session menu's Share session…, the same picker. */}
                    {owner && session !== null && (
                      <button data-share-session onClick={act(() => shareSession(session))}>
                        Share session…
                      </button>
                    )}
                    {active !== undefined &&
                      owner &&
                      client.info(active)?.type === "doc" &&
                      docShareItems(client, active).map((i) =>
                        typeof i === "string" || "header" in i ? null : (
                          <button key={i.label} data-doc-share={i.label} disabled={i.disabled} title={i.hint} onClick={act(i.run)}>
                            {i.label}
                          </button>
                        ),
                      )}
                    {active !== undefined && (
                      <button class="danger" onClick={act(() => client.intent({ op: "close_pane", pane: active }))}>
                        Close pane
                      </button>
                    )}
                    {closeTab && (
                      <button class="danger" onClick={act(() => client.tab !== null && client.intent({ op: "close_tab", tab: client.tab }))}>
                        {client.tabMachine(client.tab!) ? "Close tab and machine" : "Close tab"}
                      </button>
                    )}
                  </div>
                </section>
              )}
              <section class="sheet-group">
                <h2>Open</h2>
                <div class="sheet-actions">
                  <button data-open-swarm onClick={act(openSwarm)}>
                    Swarm
                  </button>
                  <button data-open-palette onClick={act(() => openPalette(client, true))}>
                    Commands
                  </button>
                  {owner && client.has("studio") && (
                    <button data-studio-apps onClick={act(() => pickApp(client, { session: where }, true))}>
                      Studio apps
                    </button>
                  )}
                  {owner && (
                    <button data-open-pr onClick={act(() => void openPr(client, { session: where }))}>
                      Pull request
                    </button>
                  )}
                  {owner && (
                    <button data-open-issue onClick={act(() => void openIssue(client, { session: where }))}>
                      Issue
                    </button>
                  )}
                  {owner && client.flag("agents") && (
                    <button data-open-agents onClick={act(() => openAgentsPage())}>
                      Agents
                    </button>
                  )}
                  {owner && client.flag("docs") && (
                    <button data-open-docs onClick={act(() => openDocsPage())}>
                      Docs
                    </button>
                  )}
                  {active !== undefined && owner && (
                    <button data-changes onClick={act(() => openChanges(client, active))}>
                      Changes
                    </button>
                  )}
                  {active !== undefined && (
                    // Where the active pane runs: its machine, or this host.
                    <button onClick={act(() => void openPort(client, { split: active, host: client.machine(active)?.id, local: !client.machine(active) }))}>
                      Open port
                    </button>
                  )}
                  {client.has("vms") && session !== null && (
                    <button onClick={act(() => void client.newVm({ session, tab: true }))}>New VM tab</button>
                  )}
                  {client.has("vms") && <button onClick={act(() => openSandboxes(client))}>Sandboxes</button>}
                  <button onClick={act(() => client.intent({ op: "new_session", name: null, from_pane: active ?? null }))}>New session</button>
                </div>
              </section>
              <section class="sheet-group sheet-you">
                <h2>You</h2>
                <NotifySection client={client} session={session} />
                <AccountSection close={close} />
                <div class="sheet-actions">
                  <button data-getting-started-open onClick={act(() => openGettingStarted(undefined, client))}>
                    Getting started
                  </button>
                  {owner && <button onClick={act(() => openMachineTelemetry(client))}>Usage and crash reports</button>}
                  {owner && client.hasDev() && <button onClick={act(() => openDeveloper(client))}>Developer settings</button>}
                </div>
              </section>
            </>
          )}
        </div>
        <div class="sheet-tabs" role="tablist" aria-label="Sheet">
          {TABS.map(([id, label]) => (
            <button
              key={id}
              role="tab"
              id={`sheet-tab-${id}`}
              aria-selected={tab === id}
              aria-controls={`sheet-panel-${id}`}
              tabIndex={tab === id ? 0 : -1}
              data-sheet-tab={id}
              onClick={() => setTab(id)}
              onKeyDown={(e) => {
                const to = TABS[TABS.findIndex(([t]) => t === tab) + (e.key === "ArrowRight" ? 1 : e.key === "ArrowLeft" ? -1 : 0)];
                if (!to || to[0] === tab) return;
                setTab(to[0]);
                document.getElementById(`sheet-tab-${to[0]}`)?.focus();
              }}
            >
              {label}
              {id === "here" && wanting.length > 0 && <span class="sheet-tab-dot" role="img" aria-label="needs you" />}
            </button>
          ))}
        </div>
      </nav>
    </div>
  );
}

/** #820: the account menu's items (the bar's, on a desktop), so a phone
 * can remove a machine or a device, manage a team and reach sign-in. None
 * on a daemon's own page. */
function AccountSection({ close }: { close: () => void }) {
  const account = useAccount();
  if (!account) return null;
  const run = (fn: () => void) => () => {
    close();
    fn();
  };
  return (
    <section class="sheet-account" data-sheet-account>
      <h2>{accountHeader(account.login())}</h2>
      <div class="sheet-actions">
        {account.items().flatMap((item) =>
          typeof item === "object" && "run" in item
            ? [
                <button key={item.label} aria-label={item.label} disabled={item.disabled} onClick={run(item.run)}>
                  {item.label}
                </button>,
              ]
            : [],
        )}
        <button data-sheet-sign-out aria-label="Sign out" onClick={run(() => account.signOut())}>
          Sign out
        </button>
      </div>
    </section>
  );
}

const KEYS: { label: string; bytes?: string; app?: string; mod?: "ctrl" | "alt" }[] = [
  { label: "Esc", bytes: "\x1b" },
  { label: "Tab", bytes: "\t" },
  { label: "Ctrl", mod: "ctrl" },
  { label: "Alt", mod: "alt" },
  { label: "←", bytes: "\x1b[D", app: "\x1bOD" },
  { label: "↑", bytes: "\x1b[A", app: "\x1bOA" },
  { label: "↓", bytes: "\x1b[B", app: "\x1bOB" },
  { label: "→", bytes: "\x1b[C", app: "\x1bOC" },
  { label: "|", bytes: "|" },
  { label: "~", bytes: "~" },
  { label: "/", bytes: "/" },
  { label: "-", bytes: "-" },
];

export function KeyBar({ client }: { client: Client }) {
  useSubscribe((fn) => client.subscribe(fn));
  const enc = new TextEncoder();
  return (
    <div class="keybar" role="toolbar" aria-label="Extra keys">
      <div class="keybar-keys">
      {KEYS.map((k) => {
        const on = k.mod ? client.modifiers[k.mod] : false;
        return (
          <button
            key={k.label}
            class={on ? "key on" : "key"}
            aria-pressed={k.mod ? on : undefined}
            // Keep focus (and the on-screen keyboard) on the terminal.
            onPointerDown={(e) => e.preventDefault()}
            onClick={() => {
              const pane = client.active();
              if (pane === undefined) return;
              if (k.mod) {
                client.modifiers = { ...client.modifiers, [k.mod]: !client.modifiers[k.mod] };
                client.emit();
                return;
              }
              const view = client.panes.get(pane)?.view;
              const seq = view?.appCursor && k.app ? k.app : k.bytes!;
              client.input(pane, enc.encode(seq));
            }}
          >
            {k.label}
          </button>
        );
      })}
      </div>
      {/* M70: a photo, or the camera, into the pane. Pinned at the end,
          outside the keys that scroll, so it's always in sight. */}
      <button
        class="key attach"
        title="Attach file"
        aria-label="Attach file"
        onPointerDown={(e) => e.preventDefault()}
        onClick={() => {
          const pane = client.active();
          if (pane !== undefined) void client.attachFiles(pane);
        }}
      >
        📎
      </button>
    </div>
  );
}
