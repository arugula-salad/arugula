// The footer under the workspace (M108, #1023): the machine, the focused
// pane's repo and branch when known, the agents running on this machine
// and what needs you on every machine (the bell's count, UI audit #154),
// with the shortcuts from ui/keys.ts. And the status
// pill: the phone's connection, errors everywhere, and a hosted machine's
// box waking (M182).

import { useState } from "preact/hooks";
import type { Client } from "../../client";
import { api } from "../../control";
import { platformFetch } from "../../../shared/maintenance";
import { directory } from "../../hosts";
import { agentRunning, needYouText } from "../../../shared/needs-you";
import { keyHint } from "../keys";
import { ActivityToggle } from "./activity";
import { attentionItems, openAttention, useAttentionTick } from "./attention-drawer";
import { connection } from "./context-header";
import { BranchIcon } from "./icons";

export function StatusFooter({ client }: { client: Client }) {
  useAttentionTick(client);
  const panes = client.state?.panes ?? [];
  const active = client.active();
  const focused = active === undefined ? undefined : client.info(active);
  const repo = focused?.project?.name ?? null;
  const branch = focused?.branch ?? null;
  // Not while the link is down: it may not be so any more (M113).
  // A stopped agent isn't running (UI audit #154).
  const agents = client.connected ? panes.filter(agentRunning).length : 0;
  const shown = client.tabView();
  const needs = attentionItems(client);
  const { state } = connection(client);
  const host = directory.current || "This machine";
  // M111: the attention drawer, at this machine's first; seeing it never
  // answers it.
  const next = () => openAttention((needs.find((i) => !(i.here && i.p.id === active)) ?? needs[0]).p.key);
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
          {needYouText(needs.length)}
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
  if (client.box && (!client.connected || client.box.state === "awake")) return <WakePill client={client} />;
  const text = client.error ?? (client.connected || errorsOnly ? null : client.state ? "reconnecting…" : "connecting…");
  if (!text) return null;
  return (
    <div id="status" role="status" class={client.error ? "error" : ""}>
      {text}
    </div>
  );
}

/** M182: a hosted machine's box as control says it: waking (until its
 * daemon dials in again and the page goes on where it was, then a moment
 * of how long that took), asleep (Wake wakes it), or why it didn't wake,
 * with Rebuild (its box made again from its spec, so asked twice) and Try
 * again. */
function WakePill({ client }: { client: Client }) {
  const [armed, setArmed] = useState(false);
  const [busy, setBusy] = useState(false);
  const box = client.box!;
  if (box.state === "awake") {
    return (
      <div id="status" role="status" class="wake" data-wake="awake" data-wake-ms={box.ms}>
        awake · {((box.ms ?? 0) / 1000).toFixed(1)} s
      </div>
    );
  }
  if (box.state === "waking") {
    return (
      <div id="status" role="status" class="wake" data-wake="waking">
        waking…
      </div>
    );
  }
  if (box.state === "asleep") {
    return (
      <div id="status" role="status" class="wake" data-wake="asleep">
        Asleep{" "}
        <button class="link" onClick={() => client.wake()} data-wake-again>
          Wake
        </button>
      </div>
    );
  }
  const hosted = client.e2e?.hosted;
  const rebuild = async () => {
    if (!hosted) return;
    if (!armed) return setArmed(true);
    setBusy(true);
    try {
      await api(`/api/projects/${hosted.project}/workspaces/${hosted.workspace}/rebuild`, {}, platformFetch);
      client.toast("Rebuilding: the workspace opens again once its new box is ready");
    } catch (e) {
      client.toast((e as Error).message);
    }
    setBusy(false);
    setArmed(false);
  };
  return (
    <div id="status" role="alert" class="wake error" data-wake="failed" data-wake-code={box.code}>
      {box.why ?? "It didn't wake."}{" "}
      {hosted && box.code !== "starting" && (
        <button class="link" disabled={busy} onClick={() => void rebuild()} data-wake-rebuild
          title="Delete its box and make it again from the workspace's spec">
          {armed ? "Rebuild: its box starts over" : "Rebuild"}
        </button>
      )}{" "}
      <button class="link" onClick={() => client.wake()} data-wake-again>
        Try again
      </button>
    </div>
  );
}
