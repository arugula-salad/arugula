// Chat blocks: a plain conversation with a model, like a chat app's. The
// daemon runs it as an agent block's session with every tool off, so this
// draws an agent block's messages and composer (markdown, images and files
// pasted, dropped or picked) and leaves out what a chat never has: the
// commands' terminal, tool calls, permission cards and the working
// directory. The model is picked in its bar. Works the same on a phone.

import { render } from "preact";
import { useLayoutEffect, useRef, useState } from "preact/hooks";
import type { Client } from "../client";
import type { PaneId } from "../../shared/proto";
import { AdapterHelp, installAdapter } from "../ui/adapter";
import { AgentMessage } from "../../shared/agent-message";
import { Composer, KeptImage, STATUS, filesOf, money, type AgentState, type Entry } from "./agent";
import { registerBlock, type BlockView } from "./view";

/** Plain text of the conversation, roughly as `capture --text` gives it. */
export function chatText(s: AgentState): string {
  const out: string[] = [];
  for (const e of s.entries) {
    if (e.type === "user") out.push(`> ${e.text}${(e.images ?? []).map((n) => ` [image ${n}]`).join("")}`);
    else if (e.type !== "tool") out.push(e.text);
  }
  return out.join("\n");
}

/** What a chat's tab is called: its title (Claude Code's, after the first
 * turn, or its first message until then). */
export function chatTitle(s: AgentState | null): string {
  return s?.title || "New chat";
}

function ModelPicker({ client, id, s }: { client: Client; id: PaneId; s: AgentState }) {
  const models = s.models ?? [];
  if (!models.length) return null;
  const current = s.model ?? "";
  return (
    <select
      class="chat-block-model"
      aria-label="Model"
      data-chat-model
      value={current}
      title={models.find((m) => m.value === current)?.description ?? "Model"}
      onChange={(e) => void client.api(`/api/blocks/${id}/call/set_model`, { model: e.currentTarget.value }, "couldn't switch the model")}
    >
      {!models.some((m) => m.value === current) && <option value={current}>{current || "Model"}</option>}
      {models.map((m) => (
        <option key={m.value} value={m.value}>
          {m.name}
        </option>
      ))}
    </select>
  );
}

function Message({ client, id, e }: { client: Client; id: PaneId; e: Entry }) {
  switch (e.type) {
    case "user":
      return (
        <div class="agent-user">
          {e.text}
          {e.images?.length ? (
            <div class="agent-images">
              {e.images.map((name) => (
                <KeptImage key={name} client={client} id={id} name={name} />
              ))}
            </div>
          ) : null}
        </div>
      );
    case "agent":
      return <AgentMessage text={e.text} />;
    case "thought":
      return <AgentMessage text={e.text} thought />;
    case "note":
      return <div class="agent-note">{e.text}</div>;
    // A chat has no tools: nothing to draw.
    case "tool":
      return null;
  }
}

function ChatBlock({ client, id, s }: { client: Client; id: PaneId; s: AgentState | null }) {
  const scroller = useRef<HTMLDivElement>(null);
  const stick = useRef(true);
  const [attached, setAttached] = useState<File[]>([]);
  useLayoutEffect(() => {
    const el = scroller.current;
    if (el && stick.current) el.scrollTop = el.scrollHeight;
  });
  if (!s) return <div class="agent-empty">Starting…</div>;
  const stopped = s.status === "stopped" || s.status === "exited";
  const said = s.entries.some((e) => e.type === "user" || e.type === "agent");
  return (
    <div
      class="agent chat-block"
      onDragOver={(e) => {
        if (e.dataTransfer?.types.includes("Files")) e.preventDefault();
      }}
      onDrop={(e) => {
        const files = filesOf(e.dataTransfer);
        if (!files.length) return;
        e.preventDefault();
        setAttached([...attached, ...files]);
      }}
    >
      <div class="agent-bar">
        {s.status !== "ready" && <span class={`agent-status ${s.status}`}>{STATUS[s.status]}</span>}
        <span class="agent-name">{chatTitle(s)}</span>
        <span class="agent-spacer" />
        {s.cost && (
          <span class="agent-cost" title={`${s.turns} turns, ${s.tokens.total} tokens`}>
            {money(s.cost.total, s.cost.currency)}
          </span>
        )}
        <ModelPicker client={client} id={id} s={s} />
        {stopped && <button onClick={() => void client.api(`/api/blocks/${id}/call/start`, {}, "couldn't start it")}>Resume</button>}
      </div>
      <div
        class="agent-log"
        ref={scroller}
        onScroll={(e) => {
          const el = e.currentTarget as HTMLDivElement;
          stick.current = el.scrollHeight - el.scrollTop - el.clientHeight < 40;
        }}
      >
        <div class="chat-block-column">
          {s.entries_from > 0 && <div class="agent-note">{s.entries_from} earlier messages: `arugula capture %{id}`</div>}
          {!said && !s.queued.length && (
            <div class="chat-block-empty">
              <h3>What's on your mind?</h3>
              <p>A conversation with {s.label}: no tools, files or shell. Attach images or files to talk about them.</p>
            </div>
          )}
          {s.entries.map((e, i) => (
            <Message key={i} client={client} id={id} e={e} />
          ))}
          {s.status === "working" && <div class="agent-working">Thinking…</div>}
          {s.queued.length > 0 && <div class="agent-note">Queued: {s.queued.join(" · ")}</div>}
        </div>
      </div>
      {s.error && <div class="agent-error">{s.error}</div>}
      {s.adapter && stopped && (
        <AdapterHelp client={client} a={s.adapter} said then="Resume once it's done." install={() => void installAdapter(client, s.adapter!.kind, { split: id })} />
      )}
      <Composer client={client} id={id} s={s} attached={attached} setAttached={setAttached} placeholder="Message Claude…" />
    </div>
  );
}

registerBlock("chat", (client, id): BlockView => {
  const host = document.createElement("div");
  host.className = "block block-agent block-chat";
  let state: AgentState | null = null;
  const draw = () => render(<ChatBlock client={client} id={id} s={state} />, host);
  draw();
  return {
    host,
    setVisible: (v) => (host.style.display = v ? "" : "none"),
    update: (s) => {
      state = s as AgentState;
      draw();
    },
    title: () => chatTitle(state),
    text: () => (state ? chatText(state) : ""),
    focus: () => host.querySelector<HTMLElement>(".agent-composer textarea")?.focus(),
    dispose: () => {
      render(null, host);
      host.remove();
    },
  };
});
