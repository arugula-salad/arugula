// The agent block's context strip (#397): the model, how full the context
// window is, and whether Claude Code compacted the conversation. The daemon
// reads them from the session's transcript (`state.context`), or Claude
// Code says them from inside (#450, the arugula-live mod: a pane's `live`),
// which wins; a terminal running Claude Code with the mod gets a slim strip
// in its header too. Where the figures come from is in its popover.

import { render, type ComponentChildren } from "preact";
import { useLayoutEffect, useMemo, useRef, useState } from "preact/hooks";
import { closeOnEscape } from "../../shared/chat/escape";
import type { AgentContext } from "../../shared/context";

export type { AgentContext };

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

/** Where the strip's figures come from, in words (#450). */
export function sourceNote(c: AgentContext): string {
  if (c.source !== "live") return "Read from the transcript Claude Code writes, after each reply.";
  const v = [c.claude_version && `Claude Code ${c.claude_version}`, c.mod_version && `arugula-live ${c.mod_version}`].filter(Boolean).join(", ");
  return `Live from Claude Code${v ? ` (${v})` : ""}, as it runs.`;
}

/** A popover under its button, from the viewport's right so a narrow pane
 *  doesn't clip it, across the screen on a phone; Escape, a click away or a
 *  resize closes it. Its `layer` is for one drawn on the page's body
 *  ([`Layer`]), out of a pane header's stacking. */
function usePop() {
  const [at, setAt] = useState<{ top: number; right: number; left?: number } | null>(null);
  const open = at !== null;
  const box = useRef<HTMLSpanElement>(null);
  const layer = useMemo(() => document.createElement("div"), []);
  const close = () => setAt(null);
  // A layout effect: listening as soon as it shows, with no tick to press
  // Escape in.
  useLayoutEffect(() => {
    if (!open) return;
    const down = (ev: PointerEvent) => {
      const t = ev.target as Node;
      if (box.current && !box.current.contains(t) && !layer.contains(t)) close();
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
  const toggle = (ev: MouseEvent) => {
    if (open) return close();
    const r = (ev.currentTarget as HTMLElement).getBoundingClientRect();
    setAt(innerWidth < 600 ? { top: r.bottom + 6, right: 12, left: 12 } : { top: r.bottom + 6, right: Math.max(8, innerWidth - r.right) });
  };
  const style: Record<string, string | number> | undefined = at
    ? { top: `${at.top}px`, right: `${at.right}px`, ...(at.left != null ? { left: `${at.left}px`, minWidth: 0, width: "auto" } : {}) }
    : undefined;
  // A pane header drags on a press: one in the popover isn't that.
  const stop = (e: Event) => e.stopPropagation();
  return { open, box, layer, toggle, style, stop };
}

/** Its children on the page's body, in `host`: a terminal's header sits
 *  under the panes' frames, so a popover in it would be drawn beneath them. */
function Layer({ host, children }: { host: HTMLElement; children: ComponentChildren }) {
  useLayoutEffect(() => {
    document.body.appendChild(host);
    return () => {
      render(null, host);
      host.remove();
    };
  }, [host]);
  useLayoutEffect(() => render(<>{children}</>, host));
  return null;
}

/** `slim`: a terminal's header, without the model (its popover has it). */
export function ContextStrip({ c, slim = false }: { c: AgentContext; slim?: boolean }) {
  const pop = usePop();
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
  const fill = c.tokens == null ? null : `${about}${tokens(c.tokens)}${c.window == null ? "" : ` of ${tokens(c.window)}`} tokens`;
  const wrap = (el: preact.JSX.Element) => (slim ? <Layer host={pop.layer}>{el}</Layer> : el);
  const marked = m && `${m.trigger === "window" ? "Window exceeded" : m.trigger === "manual" ? "Compacted by hand" : "Compacted automatically"} ${new Date(m.at_ms).toLocaleString()}${before}`;
  return (
    <span class={slim ? "agent-context slim" : "agent-context"} data-context data-source={c.source ?? "transcript"} ref={pop.box}>
      <button class={pop.open ? "agent-context-btn on" : "agent-context-btn"} aria-expanded={pop.open} aria-haspopup="dialog" onClick={pop.toggle}>
        {c.model && !slim && <span class="agent-context-model">{c.model}</span>}
        {c.tokens != null && (
          <span class="agent-context-fill" data-tone={pct == null ? "unknown" : tone(pct)} data-fill={pct ?? ""} title={`${fill}${estimate}${note ? `. ${note}` : ""}`}>
            <span class="agent-context-bar">{pct != null && <span style={{ width: `${pct}%` }} />}</span>
            {pct != null ? `${pct}%` : tokens(c.tokens)}
          </span>
        )}
        {m && (
          <span class={`agent-context-mark ${m.kind}`} data-mark={m.kind} title={marked!}>
            {label}
            {c.compactions > 1 ? ` ×${c.compactions}` : ""}
          </span>
        )}
      </button>
      {pop.open && wrap(
        <div class="agent-estimate-pop" role="dialog" aria-label="Context window" data-context-pop style={pop.style} onPointerDown={pop.stop} onDblClick={pop.stop}>
          <table>
            <tbody>
              {c.model && (
                <tr>
                  <th>Model</th>
                  <td>
                    {c.model}
                    {c.effort ? `, ${c.effort} effort` : ""}
                  </td>
                </tr>
              )}
              {fill && (
                <tr>
                  <th>Context</th>
                  <td>
                    {fill}
                    {pct != null ? ` (${pct}%)` : ""}
                  </td>
                </tr>
              )}
              {note && (
                <tr>
                  <th>Window</th>
                  <td class="agent-estimate-tokens">{note}.</td>
                </tr>
              )}
              {m && (
                <tr>
                  <th>{m.kind === "overflow" ? "Overflow" : "Compacted"}</th>
                  <td class="agent-estimate-tokens">
                    {marked}
                    {c.compactions > 1 ? `; ${c.compactions} times in all` : ""}.
                  </td>
                </tr>
              )}
              {c.cost_usd != null && (
                <tr>
                  <th>Cost</th>
                  <td>
                    {usd(c.cost_usd)} <span class="agent-estimate-note">Claude Code's figure</span>
                  </td>
                </tr>
              )}
            </tbody>
          </table>
          {c.estimated && <p class="agent-estimate-lede">The context is estimated until the next turn: what the compaction kept, and what every request carries.</p>}
          <p class="agent-estimate-lede" data-context-source={c.source ?? "transcript"}>
            {sourceNote(c)}
          </p>
        </div>,
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

/** What Claude Code's own figure is, for a title or a note (#450). */
export const CLAUDE_COST_NOTE =
  "Claude Code's own figure for this session, from inside it. Not a bill: a Claude subscription isn't charged per token.";

/** The strip's cost: Claude Code's own figure when it said one (#450),
 *  else the session's estimate; the popover has the estimate's parts. */
export function CostEstimate({ e, claude }: { e?: Estimate | null; claude?: number | null }) {
  const pop = usePop();
  const own = claude != null;
  if (!e && !own) return null;
  const subs = e?.subagents.length ?? 0;
  const title = own
    ? CLAUDE_COST_NOTE
    : `${COST_NOTE}${subs ? ` Includes ${subs} subagent${subs > 1 ? "s" : ""}.` : ""}${e!.unpriced.length ? ` ${noPrice(e!)}.` : ""}`;
  return (
    <span class="agent-estimate" ref={pop.box} data-estimate={e?.usd ?? ""} data-cost={own ? "claude" : "estimate"}>
      <button class={pop.open ? "agent-estimate-btn on" : "agent-estimate-btn"} aria-expanded={pop.open} aria-haspopup="dialog" title={title} onClick={pop.toggle}>
        {own ? usd(claude!) : costLabel(e!)}
        <span class="agent-estimate-note">{own ? " Claude Code's figure" : e!.usd == null ? " · no price" : " at API prices"}</span>
      </button>
      {pop.open && (
        <div class="agent-estimate-pop" role="dialog" aria-label={own ? "Cost" : "Estimated cost"} data-estimate-pop style={pop.style} onPointerDown={pop.stop} onDblClick={pop.stop}>
          {own && <p class="agent-estimate-lede">{CLAUDE_COST_NOTE}</p>}
          {own && e && <p class="agent-estimate-lede">Its tokens, at API list prices:</p>}
          {!own && <p class="agent-estimate-lede">{COST_NOTE}</p>}
          {e && (
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
          )}
          {e && e.unpriced.length > 0 && <p class="agent-estimate-lede">{noPrice(e)}: its tokens are counted, not priced.</p>}
        </div>
      )}
    </span>
  );
}
