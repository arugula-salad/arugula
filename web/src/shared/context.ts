// What the daemon says of a Claude Code session's context window (#397):
// an agent block's `state.context`, read from its transcript, or a pane's
// `live` (#450), what Claude Code said from inside it through the
// arugula-live mod. The strip that draws it is shell/blocks/agent-context.tsx.

export interface AgentContext {
  /** Shortened for display: `Opus 5.5`. */
  model: string | null;
  /** Tokens the last assistant entry held; null until it has said. */
  tokens: number | null;
  /** `tokens` is an estimate (#444): right after a compaction, what it kept
   *  plus what every request carries, until the next turn says. */
  estimated?: boolean;
  /** Null when it can't be told (#444): a model whose 1M window is an
   *  option the transcript doesn't record. */
  window: number | null;
  /** `engine`: Claude Code said; `model`: Claude Code's default for the
   *  model; `usage`: it has held more than 200k. */
  window_from?: "engine" | "model" | "usage" | null;
  compactions: number;
  overflows: number;
  last: {
    /** `compacted`, or `overflow`: the window was full or exceeded. */
    kind: "compacted" | "overflow";
    /** `auto`, `manual`, or `window` (exceeded, not compacted since). */
    trigger: string;
    at_ms: number;
    tokens_before: number | null;
    window_before: number | null;
  } | null;
  /** Where the figures come from (#450): `live` from Claude Code (its mod),
   *  or read from the `transcript`. */
  source?: "live" | "transcript";
  /** The last model request's effort: only Claude Code says. */
  effort?: string;
  /** Claude Code's own figure for what the session cost, in US dollars:
   *  only from inside it. */
  cost_usd?: number | null;
  claude_version?: string | null;
  mod_version?: string | null;
}
