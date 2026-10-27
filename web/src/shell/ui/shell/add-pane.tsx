// Add Pane: a drawer that says what kind
// of pane, on which machine, in which directory and on which side of the
// active pane, shows where that is before it makes it, and then gives the
// new pane the keys. Each kind is made the way its menu item makes it (a
// split, the agent dialog, an editor, changes, a port or web page block,
// or the pull request, issue and conversation openers of *Open beside*);
// a block opens beside the active pane and, placed below, is moved there.

import { useEffect, useRef, useState } from "preact/hooks";
import { paneIds, type Client } from "../../client";
import { newRemote, openIssue, openPr, remoteHosts } from "../../blocks";
import { directory } from "../../hosts";
import type { Node, OpenRequest, PaneId, TabView } from "../../../shared/proto";
import { startAgent } from "../agent-dialog";
import { pickConversation } from "../conversations";
import { useEscape } from "../keys";
import { openPicker } from "../picker";
import { closeAttention } from "./attention-drawer";
import { ordinal } from "../panes/layout";

// ---- whether it's open (the toolbar's button and Ctrl+Shift+E)

let open = false;
let back: HTMLElement | null = null;
/** Toolbars on the page: Ctrl+Shift+E only takes the key while one is. */
let toolbars = 0;
const listeners = new Set<() => void>();
const changed = () => listeners.forEach((fn) => fn());

export const addPane = {
  toggle() {
    if (open) return addPane.close();
    closeAttention(false);
    back = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    open = true;
    changed();
  },
  close(restore = true) {
    if (!open) return;
    open = false;
    changed();
    if (restore && back?.isConnected) back.focus();
    back = null;
  },
  /** Whether there's a toolbar for it to open under. */
  get available() {
    return toolbars > 0;
  },
};

export function useAddPane(): boolean {
  const [, tick] = useState(0);
  useEffect(() => {
    toolbars++;
    const fn = () => tick((t) => t + 1);
    listeners.add(fn);
    return () => {
      toolbars--;
      listeners.delete(fn);
      if (toolbars === 0) {
        open = false;
        back = null;
      }
    };
  }, []);
  return open;
}

// ---- the drawer

type Kind = "shell" | "agent" | "chat" | "editor" | "changes" | "port" | "pr" | "issue" | "web" | "conversation";
type Place = "right" | "down";
/** Where it runs: this host, the tab's machine (a VM tab), or another host
 * in the home daemon's list (a shell only). */
type Where = { kind: "here" } | { kind: "machine"; id: number; name: string } | { kind: "host"; name: string };

const KINDS: { kind: Kind; label: string; description: string }[] = [
  { kind: "shell", label: "Shell", description: "Run commands on a machine" },
  { kind: "agent", label: "Agent", description: "Start an AI agent in its own pane" },
  { kind: "chat", label: "Chat", description: "Talk with Claude: no tools or files" },
  { kind: "editor", label: "Editor", description: "Edit files in a code workspace" },
  { kind: "changes", label: "Changes", description: "Review code changes and diffs" },
  { kind: "port", label: "Port", description: "Preview an app running on a port" },
  { kind: "pr", label: "Pull request", description: "Its checks, reviews and timeline" },
  { kind: "issue", label: "Issue", description: "Read it, or put an agent on it" },
  { kind: "web", label: "Web page", description: "Any address, in a pane" },
  { kind: "conversation", label: "Claude Code conversation", description: "Pick one up from a terminal or the app" },
];

/** Kinds whose opener asks the rest (which agent, which PR): the drawer
 * hands over to it. */
const NEXT: Kind[] = ["agent", "pr", "issue", "conversation"];
/** Kinds that run on this machine only, as their menu items open them. */
const HERE: Kind[] = ["chat", "pr", "issue", "web", "conversation"];
/** Kinds with no working directory. */
const NO_DIR: Kind[] = ["port", "chat", "web"];

const whereKey = (w: Where) => (w.kind === "here" ? "here" : w.kind === "machine" ? `m${w.id}` : `h:${w.name}`);

export function AddPaneDrawer({ client, tab }: { client: Client; tab: TabView }) {
  const anchor = client.active(tab.id) ?? paneIds(tab)[0];
  const anchorMachine = anchor === undefined ? undefined : client.machine(anchor);
  const tabMachine = client.tabMachine(tab.id);
  const owner = !client.state?.roles;
  const here = directory.current || "this machine";

  const [kind, setKind] = useState<Kind>("shell");
  const [place, setPlace] = useState<Place>("right");
  const [where, setWhere] = useState<Where>(() => (tabMachine ? { kind: "machine", id: tabMachine.id, name: tabMachine.name ?? tabMachine.sprite } : { kind: "here" }));
  const [dir, setDir] = useState(() => (anchor === undefined ? "" : (client.cwd(anchor) ?? "")));
  const [port, setPort] = useState("");
  const [url, setUrl] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const first = useRef<HTMLButtonElement>(null);
  /** Stops waiting for the pane being made (it failed). */
  const waiting = useRef<(() => void) | null>(null);
  useEscape("add-pane.close", busy ? null : () => addPane.close(), [busy]);
  useEffect(() => first.current?.focus(), []);

  // What each kind may be, here and for this person (as the pane menu
  // offers them).
  const kindRefusal = (k: Kind): string | null => {
    if (k === "editor" && !owner) return "Editors are the machine owner's";
    if (k === "changes" && !owner) return "Changes are the machine owner's";
    if ((k === "pr" || k === "issue") && !owner) return "Pull requests and issues open with the machine owner's login";
    if (k === "conversation" && !owner) return "Conversations are the machine owner's";
    if ((k === "editor" || k === "port") && !client.showsBlocks()) return "This page can't show block sites";
    return null;
  };
  const wheres: Where[] = [
    { kind: "here" },
    ...(tabMachine ? [{ kind: "machine", id: tabMachine.id, name: tabMachine.name ?? tabMachine.sprite } as Where] : []),
    ...remoteHosts().map((name): Where => ({ kind: "host", name })),
  ];
  const whereRefusal = (w: Where): string | null => {
    if (w.kind === "host" && kind !== "shell") return "Only a shell opens on another host from here";
    if (w.kind !== "here" && kind === "agent") return "The agent dialog says where an agent runs";
    if (w.kind !== "here" && HERE.includes(kind)) return `${KINDS.find((k) => k.kind === kind)!.label} opens on this machine`;
    return null;
  };
  const chosenWhere = whereRefusal(where) ? { kind: "here" as const } : where;
  // The picker browses the active pane's machine: a directory means
  // something only where that pane runs.
  const dirApplies =
    anchor !== undefined &&
    ((chosenWhere.kind === "here" && !anchorMachine) || (chosenWhere.kind === "machine" && anchorMachine?.id === chosenWhere.id));
  const theDir = dirApplies ? dir.trim() : "";
  const portMatch = port.trim().match(/^:?(\d{1,5})(\/.*)?$/);

  const whereName = (w: Where) => (w.kind === "here" ? here : w.kind === "machine" ? `VM ${w.name}` : w.name);
  const anchorName = anchor === undefined ? "" : `pane ${ordinal(tab, anchor)} (${client.title(anchor) || client.info(anchor)?.command || `%${anchor}`})`;
  const label = KINDS.find((k) => k.kind === kind)!.label;
  const what = kind === "port" ? `Port ${portMatch ? portMatch[1] : "…"}` : label;
  const destination = `${what} on ${whereName(chosenWhere)}${NO_DIR.includes(kind) ? "" : ` in ${theDir || "its home directory"}`}, ${place === "right" ? "right of" : "below"} ${anchorName}`;
  const refusal =
    anchor === undefined
      ? "No pane to put it beside"
      : (kindRefusal(kind) ?? (kind === "port" && !portMatch ? "Type a port number" : kind === "web" && !url.trim() ? "Type an address" : null));

  /** Once the new pane shows in this tab: move it below if it was opened
   * beside, give it the keys, and close. */
  const settle = (before: Set<PaneId>, moveDown: boolean, quiet = false) => {
    const started = Date.now();
    const look = () => {
      const t = client.tabView(tab.id);
      const made = t && paneIds(t).find((id) => !before.has(id));
      if (made !== undefined) {
        stop();
        if (moveDown && anchor !== undefined) client.intent({ op: "move_pane", pane: made, target: anchor, edge: "bottom" });
        client.setActive(made);
        requestAnimationFrame(() => client.viewOf(made)?.focus());
        if (!quiet) {
          setBusy(false);
          addPane.close(false);
        }
      } else if (Date.now() - started > 20_000) {
        stop();
        if (!quiet) {
          setBusy(false);
          setError("The new pane didn't show up");
        }
      }
    };
    const unsub = client.subscribe(look);
    const timer = window.setInterval(look, 1000);
    const stop = () => {
      unsub();
      clearInterval(timer);
    };
    if (!quiet) waiting.current = stop;
    look();
  };

  const create = async () => {
    if (refusal || anchor === undefined || busy) return;
    setError(null);
    const before = new Set(paneIds(tab));
    const failed = (m: string) => {
      waiting.current?.();
      setBusy(false);
      setError(m.charAt(0).toUpperCase() + m.slice(1));
    };
    const on = chosenWhere.kind === "machine" ? { host: chosenWhere.id } : tabMachine ? { local: true } : {};
    if (kind === "agent") {
      // The agent dialog is how an agent starts (which one, its prompt):
      // it takes over from here.
      addPane.close();
      settle(before, place === "down", true);
      startAgent(client, { split: anchor, from: anchor, cwd: theDir || undefined });
      return;
    }
    if (kind === "pr" || kind === "issue" || kind === "conversation") {
      // As *Open beside* opens them: their own question says which.
      addPane.close();
      settle(before, place === "down", true);
      if (kind === "conversation") pickConversation(client, { split: anchor, cwd: theDir || undefined });
      else void (kind === "pr" ? openPr : openIssue)(client, { split: anchor, dir: theDir || null });
      return;
    }
    setBusy(true);
    if (kind === "shell") {
      if (chosenWhere.kind === "host") {
        const was = client.error;
        await newRemote(client, chosenWhere.name, { split: anchor });
        if (client.error && client.error !== was) return failed(client.error);
        settle(before, place === "down");
        return;
      }
      settle(before, false);
      client.intent(
        {
          op: "split",
          pane: anchor,
          edge: place === "right" ? "right" : "bottom",
          ...(tabMachine ? { local: chosenWhere.kind === "here" } : {}),
          ...(theDir ? { cwd: theDir } : {}),
        },
        failed,
      );
      return;
    }
    const body: OpenRequest =
      kind === "chat"
        ? { type: "chat", config: { agent: "claude" }, split: anchor, from_pane: anchor, local: true }
        : kind === "editor"
        ? { type: "editor", config: theDir ? { path: theDir } : {}, split: anchor, from_pane: anchor, ...on }
        : kind === "changes"
          ? { type: "diff", config: theDir ? { repo: theDir } : {}, split: anchor, from_pane: anchor, ...on }
          : kind === "web"
            ? { type: "browser", config: { url: url.trim() }, split: anchor }
            : { type: "browser", config: { port: Number(portMatch![1]), path: portMatch![2] ?? "/" }, split: anchor, ...on };
    const err = await client.make("/api/blocks", body);
    if (err) return failed(err);
    settle(before, place === "down");
  };

  return (
    <div
      class="add-pane"
      data-add-pane-drawer
      role="dialog"
      aria-label="Add a pane"
      onKeyDown={(e) => {
        if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
          e.preventDefault();
          void create();
        }
      }}
    >
      <header class="add-pane-head"><div><h2>Add a pane</h2><p>{tab.name || "Current tab"} · {anchorName}</p></div><button aria-label="Close Add Pane" disabled={busy} onClick={() => addPane.close()}>✕</button></header>
      <div class="add-pane-body">
      <div class="add-pane-row">
        <span class="add-pane-label" id="add-pane-kind">
          Pane type
        </span>
        <div class="add-pane-seg add-pane-kinds" role="radiogroup" aria-labelledby="add-pane-kind">
          {KINDS.map(({ kind: k, label, description }, i) => (
            <button
              key={k}
              ref={i === 0 ? first : undefined}
              role="radio"
              aria-checked={kind === k}
              data-kind={k}
              aria-label={label}
              disabled={!!kindRefusal(k)}
              title={kindRefusal(k) ?? undefined}
              onClick={() => setKind(k)}
            >
              <KindIcon kind={k} /><span><strong>{label}</strong><small>{description}</small></span>
            </button>
          ))}
        </div>
      </div>
      {kind === "port" && (
        <label class="add-pane-row">
          <span class="add-pane-label">Port</span>
          <input
            class="add-pane-input"
            data-add-pane-port
            value={port}
            placeholder="port number, optionally /path"
            autocomplete="off"
            onInput={(e) => setPort(e.currentTarget.value)}
          />
        </label>
      )}
      <label class="add-pane-row">
        <span class="add-pane-label">Machine</span>
        <select
          class="add-pane-input"
          data-add-pane-machine
          value={whereKey(chosenWhere)}
          onChange={(e) => {
            const w = wheres.find((x) => whereKey(x) === e.currentTarget.value);
            if (w) setWhere(w);
          }}
        >
          {wheres.map((w) => (
            <option key={whereKey(w)} value={whereKey(w)} disabled={!!whereRefusal(w)}>
              {whereName(w)}
              {w.kind === "here" ? " (this machine)" : w.kind === "machine" ? " (the tab's)" : ""}
            </option>
          ))}
        </select>
      </label>
      {kind === "web" && (
        <label class="add-pane-row">
          <span class="add-pane-label">Address</span>
          <input
            class="add-pane-input"
            data-add-pane-url
            value={url}
            placeholder="https://… or example.com"
            autocomplete="off"
            onInput={(e) => setUrl(e.currentTarget.value)}
          />
        </label>
      )}
      {!NO_DIR.includes(kind) && (
        <div class="add-pane-row">
          <span class="add-pane-label">Working directory</span>
          <div class="add-pane-directory"><span class="add-pane-dir" data-add-pane-dir title={theDir || undefined}>
            {dirApplies ? theDir || "home" : "home (on that machine)"}
          </span>
          <button
            class="add-pane-choose"
            disabled={!dirApplies}
            title={dirApplies ? "Choose a directory" : "The picker browses the active pane's machine"}
            onClick={() => anchor !== undefined && openPicker(client, anchor, false, setDir)}
          >
            Choose…
          </button></div>
        </div>
      )}
      <div class="add-pane-row">
        <span class="add-pane-label" id="add-pane-place">
          Placement
        </span>
        <div class="add-pane-seg add-pane-placements" role="radiogroup" aria-labelledby="add-pane-place">
          {(["right", "down"] as const).map((p) => (
            <button key={p} role="radio" aria-checked={place === p} data-place={p} aria-label={p === "right" ? "Right" : "Down"} onClick={() => setPlace(p)}>
              <span class={`add-pane-placement-icon ${p}`} aria-hidden="true" /><span><strong>{p === "right" ? "Right" : "Down"}</strong><small>{p === "right" ? "Beside the active pane" : "Below the active pane"}</small></span>
            </button>
          ))}
        </div>
      </div>
      <div class="add-pane-row"><span class="add-pane-label">Layout preview</span><div class="add-pane-preview" aria-label={`New pane ${place === "right" ? "right of" : "below"} ${anchorName}`}><LayoutPreview node={tab.root} anchor={anchor} place={place} label={label} /></div></div>
      <p class="add-pane-dest" data-add-pane-destination>
        {destination}
        {kind === "agent" && <span class="add-pane-then"> (the agent dialog opens next)</span>}
        {(kind === "pr" || kind === "issue") && <span class="add-pane-then"> (it asks which {kind === "pr" ? "pull request" : "issue"} next)</span>}
        {kind === "conversation" && <span class="add-pane-then"> (the conversations open next, to pick one)</span>}
      </p>
      {error && (
        <p class="add-pane-error" role="alert" data-add-pane-error>
          {error}
        </p>
      )}
      </div>
      <div class="add-pane-actions">
        <button class="secondary" onClick={() => addPane.close()} disabled={busy}>
          Cancel
        </button>
        <button class="primary" data-add-pane-create disabled={!!refusal || busy} title={refusal ?? "Ctrl+Enter"} onClick={() => void create()}>
          {busy ? "Opening…" : NEXT.includes(kind) ? "Next…" : `Open ${label.toLowerCase()}`}
        </button>
      </div>
    </div>
  );
}

/** Mirror the real split tree, showing the new split only at its anchor. */
function LayoutPreview({ node, anchor, place, label }: { node: Node; anchor: PaneId | undefined; place: Place; label: string }) {
  if (node.type === "pane") return node.pane === anchor ? (
    <div class={`add-pane-preview-split ${place === "right" ? "row" : "column"}`}>
      <span class="add-pane-preview-cell active">Active</span><span class="add-pane-preview-cell new">+ {label}</span>
    </div>
  ) : <span class="add-pane-preview-cell" />;
  return <div class={`add-pane-preview-split ${node.dir}`}>{node.children.map((child, i) => <div key={i} style={{ flex: `${child.weight} 1 0`, minWidth: 0, minHeight: 0, display: "flex" }}><LayoutPreview node={child.node} anchor={anchor} place={place} label={label} /></div>)}</div>;
}

function KindIcon({ kind }: { kind: Kind }) {
  const paths: Record<Kind, string> = {
    shell: "M4 4h16v16H4z M7 9l3 3-3 3 M13 16h4",
    agent: "M5 8h14v12H5z M12 8V4 M9 4h6 M8 12h1 M15 12h1 M9 16h6",
    chat: "M4 5h16v11H9l-5 4z M8 9h8 M8 12h5",
    editor: "M6 3h8l4 4v14H6z M14 3v5h4 M9 12h6 M9 16h6",
    changes: "M7 7v10 M9 5h7v12 M7 3a2 2 0 1 0 0 4a2 2 0 1 0 0-4 M7 17a2 2 0 1 0 0 4a2 2 0 1 0 0-4 M16 17a2 2 0 1 0 0 4a2 2 0 1 0 0-4",
    port: "M3 12h18 M12 3c-6 6-6 12 0 18 M12 3c6 6 6 12 0 18 M12 3a9 9 0 1 0 0 18a9 9 0 1 0 0-18",
    pr: "M6 7v10 M6 3a2 2 0 1 0 0 4a2 2 0 1 0 0-4 M6 17a2 2 0 1 0 0 4a2 2 0 1 0 0-4 M18 17a2 2 0 1 0 0 4a2 2 0 1 0 0-4 M18 17V9a3 3 0 0 0-3-3h-4 M13 4l-2 2 2 2",
    issue: "M12 3a9 9 0 1 0 0 18a9 9 0 1 0 0-18 M12 8v5 M12 16v.5",
    web: "M3 5h18v14H3z M3 9h18 M6 7h.5 M8.5 7h.5",
    conversation: "M4 4h12v9H8l-4 3z M10 16v1h6l4 3v-9h-4",
  };
  return <svg class="add-pane-kind-icon" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d={paths[kind]} /></svg>;
}
