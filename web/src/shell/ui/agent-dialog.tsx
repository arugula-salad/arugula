// Starting an agent block: which agent, where it runs, and the first prompt.
// Opened from a pane's menu and the phone's sheet. More holds what
// `arugula agent` takes besides (Discovery 10): its permission mode
// (`--permission-mode`), MCP servers (`--mcp NAME=CMD`), tools allowed
// without asking (`--allow`) and Claude Code's own settings
// (`--user-settings`). Folded, and empty unless set: the defaults are the
// daemon's, as without them.

import { useEffect, useLayoutEffect, useRef, useState } from "preact/hooks";
import type { Client } from "../client";
import type { PaneId } from "../../shared/proto";
import { AdapterHelp, installAdapter, loadAdapters, type Adapter } from "./adapter";
import { pickConversation } from "./conversations";
import { AgentCanCard } from "./agent-can";

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

/** Claude Code's permission modes (`--permission-mode` takes another
 * agent's own too). */
const MODES = ["default", "acceptEdits", "plan", "auto"];

/** More's fields, as the block's config takes them (the CLI's). */
export function moreConfig(m: { mode: string; mcp: string; allow: string; userSettings: boolean }, kind: Kind): Record<string, unknown> {
  const out: Record<string, unknown> = {};
  if (m.mode.trim()) out.permission_mode = m.mode.trim();
  const mcp = m.mcp
    .split(/\r?\n/)
    .map((l) => l.trim())
    .filter(Boolean);
  if (mcp.length) out.mcp_servers = mcp;
  const allow = m.allow.split(/[\s,]+/).filter(Boolean);
  if (allow.length) out.allow = allow.map((tool) => ({ tool }));
  if (m.userSettings && kind === "claude") out.user_settings = true;
  return out;
}

const remembered = (): Remembered => {
  try {
    const last = JSON.parse(localStorage.getItem("arugula.agent") ?? "{}") as Omit<Remembered, "kind"> & { kind?: Kind | "fountain" | "managoat" };
    // Agents that are gone start as Claude Code.
    return { ...last, kind: last.kind === "fountain" || last.kind === "managoat" ? "claude" : last.kind };
  } catch {
    return {};
  }
};

function AgentDialog({ client, where, close }: { client: Client; where: AgentWhere; close: () => void }) {
  const last = remembered();
  const [kind, setKind] = useState<Kind>(last.kind ?? "claude");
  const [acp, setAcp] = useState(last.acp ?? "");
  const [model, setModel] = useState(last.model ?? "");
  const [vm, setVm] = useState(false);
  const [cwd, setCwd] = useState(where.cwd || (where.from !== undefined && client.cwd(where.from)) || "");
  const [prompt, setPrompt] = useState("");
  const [mode, setMode] = useState("");
  const [mcp, setMcp] = useState("");
  const [allow, setAllow] = useState("");
  const [userSettings, setUserSettings] = useState(false);
  const [mcpErr, setMcpErr] = useState("");
  const [more, setMore] = useState(false);
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
    const bad = mcp
      .split(/\r?\n/)
      .map((l) => l.trim())
      .find((l) => l && !/^[^=\s]+=\S/.test(l));
    if (bad) return (setMore(true), setMcpErr(`${bad}: want NAME=COMMAND`));
    remember();
    const config: Record<string, unknown> = { agent: kind };
    if (kind === "acp") config.command = acp.trim();
    if (model.trim()) config.model = model.trim();
    if (!(vm && canVm) && cwd.trim()) config.cwd = cwd.trim();
    if (prompt.trim()) config.prompt = prompt.trim();
    Object.assign(config, moreConfig({ mode, mcp, allow, userSettings }, kind));
    close();
    await client.newAgent({ config, vm: vm && canVm, split: where.split, session: where.session, from: where.from });
  };

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
        <details class="agent-more" open={more} onToggle={(e) => setMore(e.currentTarget.open)} data-agent-more>
          <summary>More</summary>
          <label>
            Permission mode
            <input name="permission_mode" list="agent-modes" value={mode} placeholder="the agent's default" onInput={(e) => setMode(e.currentTarget.value)} />
            <datalist id="agent-modes">
              {MODES.map((m) => (
                <option key={m} value={m} />
              ))}
            </datalist>
          </label>
          <label>
            MCP servers <span class="hint">NAME=COMMAND, one a line</span>
            <textarea
              name="mcp"
              rows={2}
              value={mcp}
              placeholder="files=npx -y @modelcontextprotocol/server-filesystem ."
              onInput={(e) => (setMcp(e.currentTarget.value), setMcpErr(""))}
            />
          </label>
          {mcpErr && (
            <p class="control-error" role="alert" data-agent-mcp-error>
              {mcpErr}
            </p>
          )}
          <label>
            Allow without asking <span class="hint">tools, as "always" on a card</span>
            <input name="allow" value={allow} placeholder="Read, Edit, Bash" onInput={(e) => setAllow(e.currentTarget.value)} />
          </label>
          {kind === "claude" && (
            <label class="check">
              <input type="checkbox" name="user_settings" checked={userSettings} onChange={(e) => setUserSettings(e.currentTarget.checked)} />
              With my Claude Code settings (allow and deny lists, default mode, CLAUDE.md; not their hooks)
            </label>
          )}
        </details>
        <AgentCanCard
          client={client}
          folded
          use={(p) => {
            setPrompt(p);
            promptRef.current?.focus();
          }}
        />
        <div class="prompt-buttons">
          <button class="secondary" type="button" onClick={close}>
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
