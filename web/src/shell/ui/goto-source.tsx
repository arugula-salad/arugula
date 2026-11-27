// What a machine's page puts in Go to… (shared/kit/frame/goto.tsx, Lift 3,
// #1479): this machine's sessions, tabs and panes, its Docs, its places,
// the menus' commands (25 of them, no clipboard ones: the rest are in each
// menu), the keyboard's ways out of a terminal and a search of every pane's output (native #245), as Go to…'s
// `output` mode. The palette it replaces listed every menu item.

import { useEffect } from "preact/hooks";
import { paneIds, tabLabel, type Client } from "../client";
import type { DocRow, PaneId } from "../../shared/proto";
import { addGoSource, openGoTo, type GoItem } from "../../shared/kit/frame/goto";
import { keyHint } from "../../shared/keys";
import { askText, type MenuItem } from "../../shared/ui/menu";
import { needsYou } from "../../shared/needs-you";
import { ago } from "../../shared/time";
import { useWorkspaceDir } from "../blocks";
import { directory } from "../hosts";
import { opPath, OPS } from "../ops";
import { currentAccount } from "./app-bar";
import { helpItems, newTabItems, paneItems, sessionItems, settingsItems, tabItems } from "./commands";
import { openYourWork } from "./conversations";
import { openDoc } from "./docs-route";
import { getFleet } from "./hosts";
import { openHit, searchOutput, type Asked, type OutputHit } from "./output-search";
import { paneName } from "./pane-name";
import { PROJECTS_TO, hasChat, machinesAt, openChat, openMachinesAt } from "./place-links";
import { openPlatform, platformHref } from "../platform-links";
import { openSettings } from "./settings-route";
import { focusSessionSearch } from "./shell/prefs";
import { showSession } from "./shell/session-sidebar";

/** The menus' commands Go to… lists, by their labels in the menus. */
const KEEP = new Set([
  "Split right",
  "Split down",
  "Close pane",
  "Move to new tab",
  "Pop out",
  "Start an agent…",
  "Go to directory…",
  "Attach file…",
  "Clear scrollback",
  "Share this pane…",
  "Open in editor",
  "Changes",
  "Rename tab",
  "New tab",
  "New tab in a directory…",
  "Rename session",
  "New session",
  "Share session…",
  "Close session",
  "Settings",
  "Appearance…",
  "What's new",
  "Getting started",
  "Send feedback…",
  "nvim in Arugula",
]);

/** A menu's kept items as commands, submenus' included, keyed as the
 * palette's were (`Help/What's new`), which specs and cloud's run. */
function fromMenu(group: string, items: MenuItem[]): GoItem[] {
  const kind = group === "New" ? "tab" : group.toLowerCase();
  return items.flatMap((item): GoItem[] => {
    if (typeof item !== "object") return [];
    if ("submenu" in item) return fromMenu(group, item.submenu);
    if (!("label" in item) || !KEEP.has(item.label)) return [];
    // A row that asks first in the menu asks here too, in a prompt.
    const run = item.confirm
      ? async () => {
          if ((await askText(`${item.label}? Type yes to confirm`, ""))?.trim().toLowerCase() === "yes") item.run();
        }
      : item.run;
    return [{ key: `${group}/${item.label}`, kind, title: item.label, command: true, hint: item.shortcut, disabled: item.disabled, danger: item.danger, run }];
  });
}

/** The next pane that needs you after the active one, wrapping round:
 * Go to…'s *Go to the next pane that needs you* and its key. */
export function nextNeedingYou(client: Client): PaneId | undefined {
  const pane = client.active();
  // The shown project's only (Arugula #1347).
  const wanting = client
    .shownPanes()
    .filter((p) => {
      const i = client.info(p);
      return !!i && needsYou(i);
    })
    .sort((a, b) => a - b);
  return wanting.find((p) => p > (pane ?? -1)) ?? wanting[0];
}

/** Everything for what's shown now. */
function items(client: Client, phone: boolean, workspace: string | null, docs: DocRow[]): GoItem[] {
  const state = client.state;
  if (!state) return [];
  const machine = directory.current || client.hostName || undefined;
  const out: GoItem[] = [];
  const pane = client.active();
  const tab = client.tabView();
  const session = client.session;
  // A command acts on the panes: show them first, and give the keys back
  // to the pane (a prompt the command opens takes them).
  const onPanes = (c: GoItem): GoItem => ({
    ...c,
    run: () => {
      const a = client.active();
      if (!phone && a !== undefined) client.viewOf(a)?.focus();
      return c.run();
    },
  });

  // Jumps: this machine's sessions, tabs and panes (the shown project's
  // only, Arugula #1347), and its Docs.
  const toPane = (p: PaneId) => {
    client.setActive(p);
    requestAnimationFrame(() => client.viewOf(p)?.focus());
  };
  for (const s of client.shownSessions()) {
    if (s.id !== session) out.push({ key: `Go/session ${s.id}`, kind: "session", title: s.name, machine, run: () => showSession(client, s.id) });
    for (const tid of s.tabs) {
      const t = client.tabView(tid);
      if (!t) continue;
      const label = tabLabel(client, t);
      out.push({ key: `Go/tab ${tid}`, kind: "tab", title: label, machine, also: s.name, run: () => client.selectTab(tid) });
      for (const p of paneIds(t)) {
        const info = client.info(p);
        if (!info) continue;
        const cwd = client.cwd(p)?.replace(/^\/(?:home|Users)\/[^/]+/, "~") ?? null;
        out.push({
          key: `Go/pane ${p}`,
          kind: info.type === "workspace" ? "workspace" : "pane",
          title: info.type === "workspace" ? client.title(p).replace(/ \(chant\)$/, "") : paneName(client, info),
          machine,
          state: needsYou(info) ? "asks" : undefined,
          // Found by its session and tab too, and its directory's name.
          also: [s.name, label, cwd?.split("/").pop(), info.branch].filter(Boolean).join(" "),
          run: () => toPane(p),
        });
      }
    }
  }
  for (const d of docs)
    out.push({
      key: `Docs/open ${d.id}`,
      kind: "doc",
      title: d.title,
      machine,
      run: async () => {
        const why = await openDoc(client, d.id);
        if (why) client.toast(why);
      },
    });

  // The places this page has.
  const place = (key: string, title: string, run: () => void) => out.push({ key: `Places/${key}`, kind: "place", title, machine, run });
  if (!state.roles) place("your-work", "Your work", () => openYourWork(client));
  if (machinesAt(client)) place("machines", `${machine ?? "This machine"} on Machines`, () => openMachinesAt(client));
  // Needs you is the Overview's, where there's an app (the attention drawer went in Lift 6a, #1483).
  if (platformHref(PROJECTS_TO)) place("needs-you", "Needs you", () => openPlatform(PROJECTS_TO));
  if (hasChat()) place("chat", "Chat", () => openChat());
  // M113: a way out of a terminal (which keeps Tab) to the chrome, from
  // the keyboard; Tab and the arrows go on from there.
  if (!phone) {
    const focus = (key: string, title: string, sel: string, before?: () => void) =>
      out.push({
        key: `Places/${key}`,
        kind: "keyboard",
        title,
        command: true,
        run: () => {
          before?.();
          requestAnimationFrame(() => document.querySelector<HTMLElement>(sel)?.focus());
        },
      });
    focus("focus-rail", "Move the keyboard to the rail", ".ws-rail button");
    out.push({ key: "Places/focus-sessions", kind: "keyboard", title: "Move the keyboard to the sessions (search)", command: true, run: focusSessionSearch });
    focus("focus-tabs", "Move the keyboard to the tabs", ".ws-tabs [data-tab-id][tabindex='0']");
    focus("focus-toolbar", "Move the keyboard to the pane toolbar", ".pane-toolbar button:not(:disabled)");
    focus("focus-header", "Move the keyboard to the header", ".ws-head button");
  }

  // The commands: the menus' kept ones, then this page's own.
  if (pane !== undefined) out.push(...fromMenu("Pane", paneItems(client, pane, phone, workspace)).map(onPanes));
  if (tab) {
    const rename = async () => {
      const name = await askText("Rename tab", tab.name ?? tabLabel(client, tab));
      if (name !== null) client.intent({ op: "rename_tab", tab: tab.id, name: name.trim() || null });
    };
    out.push(...fromMenu("Tab", tabItems(client, tab, () => void rename())).map(onPanes));
  }
  if (session !== null) {
    out.push(...fromMenu("New", newTabItems(client, session)).map(onPanes));
    const s = state.sessions.find((x) => x.id === session);
    const rename = async () => {
      const name = await askText("Rename session", s?.name ?? "");
      if (name?.trim() && name.trim() !== s?.name) client.intent({ op: "rename_session", session, name: name.trim() });
    };
    out.push(...fromMenu("Session", sessionItems(client, session, () => void rename())).map(onPanes));
  }
  const next = nextNeedingYou(client);
  out.push({
    key: "Swarm/next",
    kind: "pane",
    title: "Go to the next pane that needs you",
    command: true,
    hint: keyHint("pane.next-needs-you"),
    disabled: next === undefined,
    run: () => next !== undefined && toPane(next),
  });
  // The owner's, as `arugula search` is: not for someone it's shared with.
  if (!state.roles) out.push({ key: "Search/output", kind: "output", title: "Search output…", command: true, also: "find grep text printed", run: () => openGoTo("all", "", "output") });
  out.push(...fromMenu("Settings", settingsItems(client, { machine: true })));
  if (!phone) out.push({ key: "Places/settings", kind: "settings", title: "Settings", command: true, run: () => openSettings() });
  out.push(...fromMenu("Help", helpItems(client)));
  const account = currentAccount();
  if (account) out.push({ key: "Account/Sign out", kind: "account", title: "Sign out", command: true, danger: true, run: () => account.signOut() });
  // One command two menus share (Go to directory…, New tab): once.
  const seen = new Set<string>();
  return out.filter((c) => !seen.has(c.key) && !!seen.add(c.key));
}

/** A search hit as a row: its line, then where and when. */
function hitItem(client: Client, h: OutputHit): GoItem {
  const fleetPane = h.here ? undefined : getFleet()?.panes.find((p) => p.host === h.machine && p.id === h.pane);
  const info = h.here ? client.info(h.pane) : fleetPane?.info;
  const pane = h.doc ? "a Doc" : !h.open || !info ? "closed pane" : h.here ? paneName(client, info) : info.title || info.command || `pane %${h.pane}`;
  return {
    key: `Output/${h.machine}/${h.doc ?? h.pane}/${h.offset}`,
    kind: [pane, h.command ? `in ${h.command}` : null, h.at_ms ? ago(h.at_ms) : null].filter(Boolean).join(" · "),
    title: h.line.trim() || "(blank line)",
    machine: h.machine || "this machine",
    disabled: !h.doc && (!h.open || !h.pane),
    run: () => openHit(client, h),
  };
}

/** Which machines were asked, and which couldn't be. */
function askedText(asked: Asked[]): string {
  const name = (a: Asked) => (a.here ? `${a.machine || "this machine"} (this one)` : a.machine);
  const ok = asked.filter((a) => a.state === "ok").map(name);
  const bad = asked.filter((a) => a.state === "error").map((a) => `${name(a)} (${a.why})`);
  return [ok.length ? `Searched ${ok.join(", ")}` : "", bad.length ? `couldn't search ${bad.join(", ")}` : ""].filter(Boolean).join("; ");
}

/** The workspace's source, while it's up. */
export function useGoToSource(client: Client, phone: boolean) {
  const pane = client.active();
  const info = pane === undefined ? undefined : client.info(pane);
  const cwd = pane === undefined ? null : client.cwd(pane);
  const isWorkspace = useWorkspaceDir(client, pane ?? -1, info?.type === "terminal" ? cwd : null);
  useEffect(() => {
    // The Docs this machine keeps, asked once a minute at most.
    let docs: DocRow[] = [];
    let asked = 0;
    const heard = new Set<() => void>();
    const askDocs = () => {
      if (client.state?.roles || Date.now() - asked < 60_000) return;
      asked = Date.now();
      client
        .request(OPS["docs.list"].method, opPath("docs.list"))
        .then((res) => (res.ok ? res.json<DocRow[]>() : []))
        .then((v) => {
          if (!Array.isArray(v)) return;
          docs = v;
          heard.forEach((fn) => fn());
        })
        .catch(() => {});
    };
    askDocs();
    return addGoSource({
      items: (scope) => (scope === "all" ? (askDocs(), items(client, phone, isWorkspace ? cwd : null, docs)) : []),
      typed: (q) => (client.state && !client.state.roles ? [{ key: "Search/output-for", kind: "output", title: `Search output for “${q}”`, command: true, run: () => openGoTo("all", q, "output") }] : []),
      searches: [
        {
          id: "output",
          label: "Search output",
          placeholder: "Search the output of every pane",
          empty: (q, busy) => (q.length < 2 ? "Type what was printed: the output of every pane, on each machine of yours that's connected" : busy ? "Searching…" : "No output matches"),
          search: async (q) => {
            const r = await searchOutput(client, q);
            return { items: r.hits.map((h) => hitItem(client, h)), note: askedText(r.asked) };
          },
        },
      ],
      subscribe: (fn) => {
        heard.add(fn);
        const off = client.subscribe(fn);
        return () => (heard.delete(fn), off());
      },
    });
  }, [client, phone, isWorkspace, cwd]);
}
