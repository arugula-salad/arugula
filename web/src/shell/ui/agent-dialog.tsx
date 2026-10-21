// Starting an agent block: which agent, where it runs, and the first prompt.
// Opened from a pane's menu and the phone's sheet. Run on: Cloud opens the
// platform page's Cloud workspaces, where a cloud agent starts (M153, M155).

import type { ComponentChildren } from "preact";
import { useEffect, useLayoutEffect, useRef, useState } from "preact/hooks";
import { cloudPanes, useCloudPanes } from "../../shared/cloud";
import { openPlatform, platformHref } from "../platform-links";
import type { Client } from "../client";
import type { PaneId } from "../../shared/proto";
import { AdapterHelp, installAdapter, loadAdapters, type Adapter } from "./adapter";
import { pickConversation } from "./conversations";

type Kind = "claude" | "codex" | "acp";

export interface AgentWhere {
  /** Split this block (desktop), else a new tab in `session`. */
  split?: PaneId;
  session?: number;
  /** Where the request comes from: its directory is the default. */
  from?: PaneId;
  /** A directory chosen already (+ Pane's, M110), instead of `from`'s. */
  cwd?: string;
}

let open: { client: Client; where: AgentWhere } | null = null;
const listeners = new Set<() => void>();
const changed = () => listeners.forEach((fn) => fn());

/** Show the "start an agent" dialog. */
export function startAgent(client: Client, where: AgentWhere) {
  open = { client, where };
  changed();
}

export function AgentDialogLayer() {
  const [, setTick] = useState(0);
  useEffect(() => {
    const fn = () => setTick((t) => t + 1);
    listeners.add(fn);
    return () => {
      listeners.delete(fn);
    };
  }, []);
  if (!open) return null;
  const { client, where } = open;
  return (
    <AgentDialog
      client={client}
      where={where}
      close={() => {
        open = null;
        changed();
      }}
    />
  );
}

type Remembered = { kind?: Kind; acp?: string; model?: string };

const remembered = (): Remembered => {
  try {
    const last = JSON.parse(localStorage.getItem("arugula.agent") ?? "{}") as Omit<Remembered, "kind"> & { kind?: Kind | "fountain" | "managoat" };
    // Cloud agents are chosen under Run on: Cloud.
    return { ...last, kind: last.kind === "fountain" || last.kind === "managoat" ? "claude" : last.kind };
  } catch {
    return {};
  }
};

function AgentDialog({ client, where, close }: { client: Client; where: AgentWhere; close: () => void }) {
  useCloudPanes();
  const [execution, setExecution] = useState(client.state ? "local" : "cloud");
  const last = remembered();
  const [kind, setKind] = useState<Kind>(last.kind ?? "claude");
  const [acp, setAcp] = useState(last.acp ?? "");
  const [model, setModel] = useState(last.model ?? "");
  const [vm, setVm] = useState(false);
  const [cwd, setCwd] = useState(where.cwd || (where.from !== undefined && client.cwd(where.from)) || "");
  const [prompt, setPrompt] = useState("");
  const promptRef = useRef<HTMLTextAreaElement>(null);
  useLayoutEffect(() => promptRef.current?.focus(), []);
  const canVm = client.has("vms");
  // #111: whether this agent's adapter is installed here (a VM installs
  // its own).
  const [adapters, setAdapters] = useState<Adapter[] | null>(null);
  useEffect(() => {
    void loadAdapters(client).then(setAdapters);
  }, [client]);
  const adapter = vm && canVm ? undefined : adapters?.find((a) => a.kind === kind);
  const blocked = !!adapter && adapter.state !== "installed";

  const remember = () => {
    try {
      localStorage.setItem("arugula.agent", JSON.stringify({ kind, acp, model }));
    } catch {
      // private mode: nothing remembered
    }
  };

  const submit = async () => {
    if (blocked) return;
    remember();
    const config: Record<string, unknown> = { agent: kind };
    if (kind === "acp") config.command = acp.trim();
    if (model.trim()) config.model = model.trim();
    if (!(vm && canVm) && cwd.trim()) config.cwd = cwd.trim();
    if (prompt.trim()) config.prompt = prompt.trim();
    close();
    await client.newAgent({ config, vm: vm && canVm, split: where.split, session: where.session, from: where.from });
  };

  // Cloud only once control has a platform page (M155).
  const cloud = cloudPanes.enabled && !!platformHref("/workspaces");
  const executionChoice = <label>Run on<select aria-label="Run on" value={execution} onChange={(e) => setExecution(e.currentTarget.value)}><option value="local" disabled={!client.state}>This machine</option>{cloud && <option value="cloud">Cloud</option>}</select></label>;
  // S40: a cloud agent opens as a tab in a Cloud workspace, on the platform page.
  if (execution === "cloud" && cloud) return <CloudAgentLink close={close} choice={executionChoice} />;
  return (
    <div class="prompt-backdrop" role="dialog" aria-label="Start an agent" onPointerDown={(e) => e.target === e.currentTarget && close()}>
      <form
        class="prompt agent-dialog"
        aria-label="Start an agent"
        onSubmit={(e) => {
          e.preventDefault();
          void submit();
        }}
        onKeyDown={(e) => e.key === "Escape" && close()}
      >
        <h2>Start an agent</h2>
        {executionChoice}
        <p class="hint">
          Or{" "}
          <button
            type="button"
            class="link"
            onClick={() => {
              close();
              pickConversation(client, { split: where.split, session: where.session, cwd: cwd || undefined });
            }}
          >
            pick up a Claude Code conversation
          </button>{" "}
          from a terminal or the desktop app.
        </p>

        <label>
          Agent
          <select name="agent" value={kind} onChange={(e) => setKind((e.currentTarget as HTMLSelectElement).value as Kind)}>
            <option value="claude">Claude Code</option>
            <option value="codex">Codex</option>
            <option value="acp">Another ACP agent…</option>
          </select>
        </label>
        {kind === "acp" && (
          <label>
            Command
            <input name="acp" value={acp} required placeholder="gemini --acp" onInput={(e) => setAcp(e.currentTarget.value)} />
          </label>
        )}
        {canVm && (
          <label class="check">
            <input type="checkbox" name="vm" checked={vm} onChange={(e) => setVm(e.currentTarget.checked)} />
            On a new throwaway VM
          </label>
        )}
        {vm && kind === "claude" && (
          <p class="hint">Claude Code in a VM uses the token in ~/.config/arugula/claude-oauth-token (from `claude setup-token`).</p>
        )}
        {adapter && (
          <AdapterHelp
            client={client}
            a={adapter}
            then="start the agent once it's done."
            install={() => {
              remember();
              close();
              void installAdapter(client, adapter.kind, where);
            }}
          />
        )}
        {!(vm && canVm) && (
          <label>
            Working directory
            <input name="cwd" value={cwd} placeholder="home" onInput={(e) => setCwd(e.currentTarget.value)} />
          </label>
        )}
        <label>
            Model
            <input name="model" value={model} placeholder="the agent's default (e.g. haiku)" onInput={(e) => setModel(e.currentTarget.value)} />
        </label>
        <label>
          Prompt
          <textarea name="prompt" ref={promptRef} rows={4} value={prompt} placeholder="What should it do?" onInput={(e) => setPrompt(e.currentTarget.value)} />
        </label>
        <div class="prompt-buttons">
          <button type="button" onClick={close}>
            Cancel
          </button>
          <button type="submit" class="primary" disabled={blocked}>
            Start
          </button>
        </div>
      </form>
    </div>
  );
}

/** Run on: Cloud (S40, M119): a cloud agent starts as a tab in a Cloud
 * workspace, on the platform page (M153), where + Agent opens one. */
function CloudAgentLink({ close, choice }: { close: () => void; choice: ComponentChildren }) {
  return (
    <div class="prompt-backdrop" role="dialog" aria-label="Start an agent" onKeyDown={(e) => e.key === "Escape" && close()} onPointerDown={(e) => e.target === e.currentTarget && close()}>
      <form class="prompt agent-dialog cloud-launch" aria-label="Start an agent" onSubmit={(e) => (e.preventDefault(), close(), openPlatform("/workspaces"))}>
        <h2>Start an agent</h2>
        {choice}
        <p class="dim">
          It opens as a tab in a Cloud workspace: a machine in the cloud with your repositories on it, which its tabs share. Cloud
          workspaces are on the platform page: pick one there and press + Agent, or make one.
        </p>
        <div class="prompt-buttons">
          <button type="button" onClick={close}>Cancel</button>
          <button type="submit" class="primary" data-cloud-launch-link>
            Open Cloud workspaces ↗
          </button>
        </div>
      </form>
    </div>
  );
}
