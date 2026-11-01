// The agent block's context strip (#397): the model, how full the context
// window is, and whether Claude Code compacted the conversation. The
// daemon reads them from the session's transcript (`state.context`).

import { useLayoutEffect, useRef, useState } from "preact/hooks";
import { closeOnEscape } from "../../shared/chat/escape";

/** What the daemon says of a session's context window. */
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
}

/** The fill's tone: `--ok` under 60%, `--warn` to 85%, `--danger` above. */
export function tone(pct: number): "ok" | "warn" | "danger" {
  return pct < 60 ? "ok" : pct <= 85 ? "warn" : "danger";
}

const tokens = (n: number) => (n >= 1_000_000 ? `${+(n / 1_000_000).toFixed(2)}M` : n >= 1000 ? `${Math.round(n / 1000)}k` : `${n}`);

/** Where the window's size comes from, in words. */
function windowNote(c: AgentContext): string {
  if (c.window == null) return "The window isn't known: this model runs with 200k, or 1M as its [1m] variant, and the transcript doesn't say which";
  switch (c.window_from) {
    case "engine":
      return `${tokens(c.window)} window, as Claude Code reported it`;
    case "usage":
      return `${tokens(c.window)} window: it has held more than 200k`;
    case "model":
      return `${tokens(c.window)} window: Claude Code's default for this model`;
    default:
      return "";
  }
}

export function ContextStrip({ c }: { c: AgentContext }) {
  const pct = c.tokens == null || c.window == null ? null : Math.min(100, Math.round((c.tokens / c.window) * 100));
  const m = c.last;
  const before =
    m?.tokens_before == null
      ? ""
      : m.window_before == null
        ? `, ${tokens(m.tokens_before)} before`
        : `, ${tokens(m.tokens_before)} of ${tokens(m.window_before)} (${Math.round((m.tokens_before / m.window_before) * 100)}%) before`;
  const label = m && (m.kind === "overflow" ? "Overflow" : "Compacted");
  const about = c.estimated ? "About " : "";
  const estimate = c.estimated ? ". Estimated until the next turn: what the compaction kept, and what every request carries" : "";
  const note = windowNote(c);
  return (
    <span class="agent-context" data-context>
      {c.model && <span class="agent-context-model">{c.model}</span>}
      {c.tokens != null && (
        <span
          class="agent-context-fill"
          data-tone={pct == null ? "unknown" : tone(pct)}
          data-fill={pct ?? ""}
          title={`${about}${tokens(c.tokens)}${c.window == null ? "" : ` of ${tokens(c.window)}`} tokens${estimate}${note ? `. ${note}` : ""}`}
        >
          <span class="agent-context-bar">{pct != null && <span style={{ width: `${pct}%` }} />}</span>
          {pct != null ? `${pct}%` : tokens(c.tokens)}
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

// #398: what the session and its subagents cost, estimated by the daemon from
// their transcripts' token counts at Anthropic's API list prices
// (`state.estimate`). Everyone sees the same figure, labelled "at API
// prices": it's never a bill, and a Claude subscription isn't charged per
// token. A model with no list price shows its tokens and no dollars.

/** Tokens and their estimated cost; `usd` is null when a model had no price. */
export interface Cost {
  usd: number | null;
  tokens: { input: number; output: number; cache_write: number; cache_read: number; total: number };
  /** Models with tokens and no price. */
  unpriced: string[];
}

/** The daemon's estimate for a session: the total, its own, each subagent's. */
export interface Estimate extends Cost {
  session: Cost;
  subagents: { id: string; kind: string | null; description: string | null; cost: Cost }[];
  /** Where the figures come from, in words. */
  basis: string;
  /** `tokens`: priced from the transcripts' token counts. (Claude Code's
   *  own figure could be another source later.) */
  source: string;
}

/** `$4.57`, `$0.03`, `$1,204`. */
export function usd(n: number): string {
  if (n >= 1000) return `$${Math.round(n).toLocaleString("en-US")}`;
  return `$${n.toFixed(2)}`;
}

/** The figure, `≈ $4.57` (`< $0.01`), or `1.2M tokens` with no price. */
export function costLabel(c: Cost): string {
  if (c.usd == null) return `${tokens(c.tokens.total)} tokens`;
  return c.usd > 0 && c.usd < 0.005 ? "< $0.01" : `≈ ${usd(c.usd)}`;
}

/** What a figure is, for a title or a note. */
export const COST_NOTE =
  "Estimated from the transcript's token counts at Anthropic's API list prices. Not a bill: a Claude subscription isn't charged per token.";

function noPrice(c: Cost): string {
  return c.unpriced.length ? `No list price for ${c.unpriced.join(", ")}` : "";
}

function breakdown(c: Cost): string {
  const t = c.tokens;
  return `${tokens(t.input)} in, ${tokens(t.output)} out, ${tokens(t.cache_write)} cache writes, ${tokens(t.cache_read)} cache reads`;
}

/** The strip's cost: the session's estimate, opening each part's. */
export function CostEstimate({ e }: { e: Estimate }) {
  // Where the popover goes: under the button, from the viewport's right, so a
  // narrow pane doesn't clip it; across the screen on a phone.
  const [at, setAt] = useState<{ top: number; right: number; left?: number } | null>(null);
  const open = at !== null;
  const box = useRef<HTMLSpanElement>(null);
  const close = () => setAt(null);
  // A layout effect: listening as soon as it shows, with no tick to press
  // Escape in.
  useLayoutEffect(() => {
    if (!open) return;
    const down = (ev: PointerEvent) => {
      if (box.current && !box.current.contains(ev.target as Node)) close();
    };
    // Before the pane's own keys take Escape.
    const key = closeOnEscape(close);
    addEventListener("pointerdown", down, true);
    addEventListener("keydown", key, true);
    addEventListener("resize", close);
    return () => {
      removeEventListener("pointerdown", down, true);
      removeEventListener("keydown", key, true);
      removeEventListener("resize", close);
    };
  }, [open]);
  const subs = e.subagents.length;
  return (
    <span class="agent-estimate" ref={box} data-estimate={e.usd ?? ""}>
      <button
        class={open ? "agent-estimate-btn on" : "agent-estimate-btn"}
        aria-expanded={open}
        aria-haspopup="dialog"
        title={`${COST_NOTE}${subs ? ` Includes ${subs} subagent${subs > 1 ? "s" : ""}.` : ""}${e.unpriced.length ? ` ${noPrice(e)}.` : ""}`}
        onClick={(ev) => {
          if (open) return close();
          const r = (ev.currentTarget as HTMLElement).getBoundingClientRect();
          setAt(innerWidth < 600 ? { top: r.bottom + 6, right: 12, left: 12 } : { top: r.bottom + 6, right: Math.max(8, innerWidth - r.right) });
        }}
      >
        {costLabel(e)}
        <span class="agent-estimate-note">{e.usd == null ? " · no price" : " at API prices"}</span>
      </button>
      {at && (
        <div class="agent-estimate-pop" role="dialog" aria-label="Estimated cost" data-estimate-pop style={{ top: `${at.top}px`, right: `${at.right}px`, ...(at.left != null ? { left: `${at.left}px`, minWidth: 0, width: "auto" } : {}) }}>
          <p class="agent-estimate-lede">{COST_NOTE}</p>
          <table>
            <tbody>
              <tr data-estimate-row="session">
                <th>This session</th>
                <td>{costLabel(e.session)}</td>
                <td class="agent-estimate-tokens">{breakdown(e.session)}</td>
              </tr>
              {e.subagents.map((a) => (
                <tr key={a.id} data-estimate-row={a.id} title={noPrice(a.cost) || undefined}>
                  <th>
                    {a.description ?? `Subagent ${a.id}`}
                    {a.kind && <span class="agent-estimate-kind">{a.kind}</span>}
                  </th>
                  <td>{costLabel(a.cost)}</td>
                  <td class="agent-estimate-tokens">{breakdown(a.cost)}</td>
                </tr>
              ))}
              {subs > 0 && (
                <tr class="agent-estimate-total" data-estimate-row="total">
                  <th>Total</th>
                  <td>{costLabel(e)}</td>
                  <td class="agent-estimate-tokens">{`${tokens(e.tokens.total)} tokens`}</td>
                </tr>
              )}
            </tbody>
          </table>
          {e.unpriced.length > 0 && <p class="agent-estimate-lede">{noPrice(e)}: its tokens are counted, not priced.</p>}
        </div>
      )}
    </span>
  );
}
