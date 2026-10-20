// The rail down the left of the workspace (M108, #1023): Arugula's mark
// (this machine's sessions), Sessions (the sidebar), Panes, Swarm, Chat,
// Agents and Settings. It stays while the swarm, chat or agents cover the
// panes, so the way back is always in one place, and Panes goes back to the
// session's last tab and pane (the routes never change the selection).

import type { ComponentChildren } from "preact";
import { useEffect, useState } from "preact/hooks";
import type { Client } from "../../client";
import { directory } from "../../hosts";
import { onSwarmRoute, openSwarm } from "../../swarm/route";
import { openAgentsPage } from "../agents-route";
import { settingsItems } from "../commands";
import { openMenu } from "../menu";
import { chatUnread, closePlace, currentPlace, hasChat, openChat, openOverview, useChatTick, type Place } from "../places";
import { countView } from "../../telemetry";
import { AgentsIcon, ChatIcon, Mark, PanesIcon, SessionsIcon, SettingsIcon, SwarmIcon } from "./icons";
import { focusSessionSearch, setSidebarOpen, useSidebar } from "./prefs";

/** Re-render on any route change, the swarm's too (it closes without a
 * hashchange). */
function usePlace(client: Client): Place {
  useChatTick(client);
  const [, tick] = useState(0);
  useEffect(() => {
    const fn = () => tick((t) => t + 1);
    const off = onSwarmRoute(fn);
    addEventListener("hashchange", fn);
    return () => {
      off();
      removeEventListener("hashchange", fn);
    };
  }, []);
  return currentPlace();
}

export function Rail({ client }: { client: Client }) {
  const at = usePlace(client);
  const { open } = useSidebar();
  const threads = hasChat();
  const { n, mention } = threads ? chatUnread() : { n: 0, mention: false };
  // As places.tsx decides: the machine owner's, with the Labs flag.
  const agents = client.flag("agents") && !client.state?.roles;
  const host = directory.current || "this machine";
  const panes = () => {
    if (at !== "panes") closePlace(at);
    else client.viewOf(client.active() ?? -1)?.focus();
  };
  const settings = (e: MouseEvent) => {
    countView("settings");
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    openMenu({ clientX: r.right + 4, clientY: r.top, preventDefault: () => e.preventDefault() }, settingsItems(client));
  };
  return (
    <nav class="ws-rail" aria-label="Places">
      <button
        class="ws-brand"
        aria-label={`Arugula: the sessions on ${host}`}
        title={`The sessions on ${host}: their tabs, panes and what needs you`}
        data-open-overview
        onClick={() => (at === "overview" ? focusSessionSearch() : openOverview())}
      >
        <Mark />
      </button>
      <div class="ws-rail-utility">
        <RailButton
          label="Sessions"
          tip={`${open && at === "panes" ? "Hide" : "Show"} sessions sidebar on ${host}`}
          here={open && at === "panes"}
          toggle
          hook="data-open-sessions"
          onClick={() => {
            if (at !== "panes") {
              closePlace(at);
              setSidebarOpen(true);
            } else setSidebarOpen(!open);
          }}
        >
          <SessionsIcon />
        </RailButton>
      </div>
      <RailButton label="Panes" tip="Your panes: back to this session's last tab and pane" here={at === "panes"} hook="data-open-panes" onClick={panes}>
        <PanesIcon />
      </RailButton>
      <RailButton
        label="Swarm"
        aria="Swarm: every pane on all machines"
        tip="Swarm: every pane on all machines. Panes brings you back here."
        here={at === "swarm"}
        hook="data-open-swarm"
        onClick={() => at !== "swarm" && openSwarm()}
      >
        <SwarmIcon />
      </RailButton>
      {threads && (
        <RailButton
          label="Chat"
          aria={n ? `Chat, ${n} unread${mention ? ", mentioning you" : ""}` : "Chat"}
          tip="Chat: every thread, on every machine"
          here={at === "chat"}
          hook="data-open-chat"
          onClick={() => at !== "chat" && openChat()}
        >
          <ChatIcon />
          {n > 0 && <span class={mention ? "ws-rail-count mention" : "ws-rail-count"}>{mention ? `@${n}` : n}</span>}
        </RailButton>
      )}
      {agents && (
        <RailButton
          label="Agents"
          tip="Agents: this machine's recipes and your team's agents (cloud agents open in Cloud workspaces)"
          here={at === "agents"}
          hook="data-open-agents"
          onClick={() => at !== "agents" && openAgentsPage()}
        >
          <AgentsIcon />
        </RailButton>
      )}
      <div class="ws-rail-fill" />
      <RailButton label="Settings" tip="Appearance, notifications and this machine's settings" here={false} hook="data-open-settings" onClick={settings}>
        <SettingsIcon />
      </RailButton>
    </nav>
  );
}

function RailButton({
  label,
  aria,
  tip,
  here,
  toggle,
  hook,
  onClick,
  children,
}: {
  /** A switch (the sidebar), not a place: drawn as on, not as here. */
  toggle?: boolean;
  label: string;
  aria?: string;
  tip: string;
  here: boolean;
  hook: string;
  onClick: (e: MouseEvent) => void;
  children: ComponentChildren;
}) {
  return (
    <button
      class={`ws-rail-button${here ? (toggle ? " on" : " here") : ""}`}
      aria-label={aria ?? label}
      aria-pressed={toggle ? undefined : here}
      aria-expanded={toggle ? here : undefined}
      aria-controls={toggle ? "ws-session-sidebar" : undefined}
      title={tip}
      {...{ [hook]: "" }}
      onClick={onClick}
    >
      {children}
      {toggle && <span class="ws-sidebar-chevron" aria-hidden="true">{here ? "‹" : "›"}</span>}
      <span class="ws-rail-label" aria-hidden="true">
        {label}
      </span>
    </button>
  );
}
