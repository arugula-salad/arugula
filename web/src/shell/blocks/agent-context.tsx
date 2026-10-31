// The agent block's context strip (#397): the model, how full the context
// window is, and whether Claude Code compacted the conversation. The
// daemon reads them from the session's transcript (`state.context`).

/** What the daemon says of a session's context window. */
export interface AgentContext {
  /** Shortened for display: `Opus 5.5`. */
  model: string | null;
  /** Tokens the last assistant entry held; null until it has said. */
  tokens: number | null;
  window: number;
  compactions: number;
  overflows: number;
  last: {
    /** `compacted`, or `overflow`: the window was full or exceeded. */
    kind: "compacted" | "overflow";
    /** `auto`, `manual`, or `window` (exceeded, not compacted since). */
    trigger: string;
    at_ms: number;
    tokens_before: number | null;
    window_before: number;
  } | null;
}

/** The fill's tone: `--ok` under 60%, `--warn` to 85%, `--danger` above. */
export function tone(pct: number): "ok" | "warn" | "danger" {
  return pct < 60 ? "ok" : pct <= 85 ? "warn" : "danger";
}

const tokens = (n: number) => (n >= 1_000_000 ? `${+(n / 1_000_000).toFixed(2)}M` : n >= 1000 ? `${Math.round(n / 1000)}k` : `${n}`);

export function ContextStrip({ c }: { c: AgentContext }) {
  const pct = c.tokens == null ? null : Math.min(100, Math.round((c.tokens / c.window) * 100));
  const m = c.last;
  const before = m?.tokens_before == null ? "" : `, ${tokens(m.tokens_before)} of ${tokens(m.window_before)} (${Math.round((m.tokens_before / m.window_before) * 100)}%) before`;
  const label = m && (m.kind === "overflow" ? "Overflow" : "Compacted");
  return (
    <span class="agent-context" data-context>
      {c.model && <span class="agent-context-model">{c.model}</span>}
      {pct != null && (
        <span class="agent-context-fill" data-tone={tone(pct)} data-fill={pct} title={`${tokens(c.tokens!)} of ${tokens(c.window)} tokens`}>
          <span class="agent-context-bar">
            <span style={{ width: `${pct}%` }} />
          </span>
          {pct}%
        </span>
      )}
      {m && (
        <span
          class={`agent-context-mark ${m.kind}`}
          data-mark={m.kind}
          title={`${m.trigger === "window" ? "Window exceeded" : m.trigger === "manual" ? "Compacted by hand" : "Compacted automatically"} ${new Date(m.at_ms).toLocaleString()}${before}`}
        >
          {label}
          {c.compactions > 1 ? ` ×${c.compactions}` : ""}
        </span>
      )}
    </span>
  );
}
