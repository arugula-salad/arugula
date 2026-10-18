// The footer under the new workspace (M108, #1023): the machine, the focused
// pane's repo and branch when known, the agents running and what needs you
// on this machine, with the shortcuts from ui/keys.ts. And the classic
// bar's status pill, for the connection.

import type { Client } from "../../client";
import { directory } from "../../hosts";
import type { PaneInfo } from "../../proto";
import { useSubscribe } from "../hooks";
import { keyHint } from "../keys";
import { closePlace, currentPlace } from "../places";
import { ActivityToggle } from "./activity";
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
  const agents = panes.filter((p) => isAgent(p) && p.running).length;
  const needs = panes.filter((p) => p.attention === "needs_input");
  const { state } = connection(client);
  const host = directory.current || "This machine";
  // Each press goes to the next pane that wants you, from the one after
  // the focused pane; seeing it never answers it.
  const next = () => {
    const i = needs.findIndex((p) => p.id === active);
    const p = needs[(i + 1) % needs.length];
    const at = currentPlace();
    if (at !== "panes") closePlace(at);
    client.setActive(p.id);
    client.viewOf(p.id)?.focus();
  };
  const hints: [string, string | undefined][] = [
    ["Commands", keyHint("palette")],
    ["Go to pane", keyHint("pane.jump")],
    ["Go to directory", keyHint("picker")],
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
          title="Go to the next pane that needs you (this doesn't answer it)"
          onClick={next}
        >
          <span class="ws-dot needs_input" aria-hidden="true" />
          {needs.length} needs input
        </button>
      )}
      <span class="ws-foot-fill" />
      <ActivityToggle client={client} />
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
