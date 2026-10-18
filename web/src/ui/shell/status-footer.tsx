// The footer under the workspace (M108, #1023): the machine, the focused
// pane's repo and branch when known, the agents running and what needs you
// on this machine, with the shortcuts from ui/keys.ts. And the status
// pill: the phone's connection, and errors everywhere.

import type { Client } from "../../client";
import { directory } from "../../hosts";
import type { PaneInfo } from "../../proto";
import { useSubscribe } from "../hooks";
import { keyHint } from "../keys";
import { ActivityToggle } from "./activity";
import { openAttention } from "./attention-drawer";
import { connection } from "./context-header";
import { BranchIcon } from "./icons";

/** Agents: agent blocks, and terminals running one. */
const isAgent = (p: PaneInfo) => p.type === "agent" || p.kind === "agent";

export function StatusFooter({ client }: { client: Client }) {
  useSubscribe((fn) => directory.subscribe(fn));
  const panes = client.state?.panes ?? [];
  const active = client.active();
  const focused = active === undefined ? undefined : client.info(active);
  const repo = focused?.project?.name ?? null;
  const branch = focused?.branch ?? null;
  // Not while the link is down: it may not be so any more (M113).
  const agents = client.connected ? panes.filter((p) => isAgent(p) && p.running).length : 0;
  const shown = client.tabView();
  const needs = panes.filter((p) => p.attention === "needs_input");
  const { state } = connection(client);
  const host = directory.current || "This machine";
  // M111: the attention drawer, at this machine's first; seeing it never
  // answers it.
  const next = () => openAttention(`${directory.current}:${(needs.find((p) => p.id !== active) ?? needs[0]).id}`);
  // The panes' only with a tab of them to act on.
  const hints: [string, string | undefined][] = [
    ["Commands", keyHint("palette")],
    ...(shown ? ([
      ["Go to pane", keyHint("pane.jump")],
      ["Focus", keyHint("pane.focus")],
      ["Go to directory", keyHint("picker")],
    ] as [string, string | undefined][]) : []),
  ];
  return (
    <footer class="ws-foot" data-status-footer>
      <span class="ws-foot-host" title={`This machine: ${host}`}>
        <span class={`ws-dot ${state}`} aria-hidden="true" />
        {host}
      </span>
      {(repo || branch) && (
        <span class="ws-foot-repo" title="The focused pane's repository">
          <BranchIcon size={12} />
          {[repo, branch].filter(Boolean).join(" · ")}
        </span>
      )}
      {agents > 0 && (
        <span class="ws-foot-agents" data-footer-agents={agents}>
          {agents} agent{agents === 1 ? "" : "s"} running
        </span>
      )}
      {needs.length > 0 && (
        <button
          class="ws-foot-needs"
          data-footer-needs={needs.length}
          title="What needs you: see it, answer it or go to it (looking doesn't answer it)"
          onClick={next}
        >
          <span class="ws-dot needs_input" aria-hidden="true" />
          {needs.length} needs input
        </button>
      )}
      <span class="ws-foot-fill" />
      {client.session !== null && client.state?.sessions.length ? <ActivityToggle client={client} /> : null}
      <span class="ws-foot-hints" aria-label="Shortcuts">
        {hints.flatMap(([label, key]) =>
          key
            ? [
                <span key={label} class="ws-hint">
                  <kbd>{key}</kbd> {label}
                </span>,
              ]
            : [],
        )}
      </span>
    </footer>
  );
}

export function StatusPill({ client, errorsOnly }: { client: Client; errorsOnly?: boolean }) {
  const text = client.error ?? (client.connected || errorsOnly ? null : client.state ? "reconnecting…" : "connecting…");
  if (!text) return null;
  return (
    <div id="status" role="status" class={client.error ? "error" : ""}>
      {text}
    </div>
  );
}
