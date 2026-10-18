// + Pane (M110, #1025): a drawer under the pane toolbar that says what kind
// of pane, on which machine, in which directory and on which side of the
// active pane, shows where that is before it makes it, and then gives the
// new pane the keys. Each kind is made the way its menu item makes it (a
// split, the agent dialog, an editor, changes or a port block); a block
// opens beside the active pane and, placed below, is moved there.

import { useEffect, useRef, useState } from "preact/hooks";
import { paneIds, type Client } from "../../client";
import { newRemote, remoteHosts } from "../../blocks";
import { directory } from "../../hosts";
import type { OpenRequest, PaneId, TabView } from "../../proto";
import { startAgent } from "../agent-dialog";
import { useEscape } from "../keys";
import { openPicker } from "../picker";
import { ordinal } from "../panes/layout";

// ---- whether it's open (the toolbar's button and Ctrl+Shift+E)

let open = false;
/** Toolbars on the page: Ctrl+Shift+E only takes the key while one is. */
let toolbars = 0;
const listeners = new Set<() => void>();
const changed = () => listeners.forEach((fn) => fn());

export const addPane = {
  toggle() {
    open = !open;
    changed();
  },
  close() {
    if (!open) return;
    open = false;
    changed();
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
      if (toolbars === 0) open = false;
    };
  }, []);
  return open;
}

// ---- the drawer

type Kind = "shell" | "agent" | "editor" | "changes" | "port";
type Place = "right" | "down";
/** Where it runs: this host, the tab's machine (a VM tab), or another host
 * in the home daemon's list (a shell only). */
type Where = { kind: "here" } | { kind: "machine"; id: number; name: string } | { kind: "host"; name: string };

const KINDS: { kind: Kind; label: string }[] = [
  { kind: "shell", label: "Shell" },
  { kind: "agent", label: "Agent" },
  { kind: "editor", label: "Editor" },
  { kind: "changes", label: "Changes" },
  { kind: "port", label: "Port" },
];

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
  const destination = `${what} on ${whereName(chosenWhere)}${kind === "port" ? "" : ` in ${theDir || "its home directory"}`}, ${place === "right" ? "right of" : "below"} ${anchorName}`;
  const refusal =
    anchor === undefined
      ? "No pane to put it beside"
      : (kindRefusal(kind) ?? (kind === "port" && !portMatch ? "Type a port number" : null));

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
          addPane.close();
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
      kind === "editor"
        ? { type: "editor", config: theDir ? { path: theDir } : {}, split: anchor, from_pane: anchor, ...on }
        : kind === "changes"
          ? { type: "diff", config: theDir ? { repo: theDir } : {}, split: anchor, from_pane: anchor, ...on }
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
      <div class="add-pane-row">
        <span class="add-pane-label" id="add-pane-kind">
          Type
        </span>
        <div class="add-pane-seg" role="radiogroup" aria-labelledby="add-pane-kind">
          {KINDS.map(({ kind: k, label }, i) => (
            <button
              key={k}
              ref={i === 0 ? first : undefined}
              role="radio"
              aria-checked={kind === k}
              data-kind={k}
              disabled={!!kindRefusal(k)}
              title={kindRefusal(k) ?? undefined}
              onClick={() => setKind(k)}
            >
              {label}
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
      {kind !== "port" && (
        <div class="add-pane-row">
          <span class="add-pane-label">Directory</span>
          <span class="add-pane-dir" data-add-pane-dir title={theDir || undefined}>
            {dirApplies ? theDir || "home" : "home (on that machine)"}
          </span>
          <button
            class="add-pane-choose"
            disabled={!dirApplies}
            title={dirApplies ? "Choose a directory" : "The picker browses the active pane's machine"}
            onClick={() => anchor !== undefined && openPicker(client, anchor, false, setDir)}
          >
            Choose…
          </button>
        </div>
      )}
      <div class="add-pane-row">
        <span class="add-pane-label" id="add-pane-place">
          Place
        </span>
        <div class="add-pane-seg" role="radiogroup" aria-labelledby="add-pane-place">
          {(["right", "down"] as const).map((p) => (
            <button key={p} role="radio" aria-checked={place === p} data-place={p} onClick={() => setPlace(p)}>
              {p === "right" ? "Right" : "Down"}
            </button>
          ))}
        </div>
      </div>
      <p class="add-pane-dest" data-add-pane-destination>
        {destination}
        {kind === "agent" && <span class="add-pane-then"> (the agent dialog opens next)</span>}
      </p>
      {error && (
        <p class="add-pane-error" role="alert" data-add-pane-error>
          {error}
        </p>
      )}
      <div class="add-pane-actions">
        <button onClick={() => addPane.close()} disabled={busy}>
          Cancel
        </button>
        <button class="primary" data-add-pane-create disabled={!!refusal || busy} title={refusal ?? "Ctrl+Enter"} onClick={() => void create()}>
          {busy ? "Opening…" : kind === "agent" ? "Next…" : `Open ${label.toLowerCase()}`}
        </button>
      </div>
    </div>
  );
}
