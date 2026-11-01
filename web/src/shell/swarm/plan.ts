// The swarm as a plan board (#445): not panes, but the work inside agent
// sessions, from the task list each keeps with its todo and task tools
// (plan-model.ts sorts them). It reads flat, like the hive, so it pans,
// zooms and pinches the same, a card opens its pane and hovering one peeks
// at it. Every channel is one thing a session reports:
//
// - the column is the step's status; Review is a step in progress while
//   its session waits on its person, Blocked one waiting on another step;
// - a card's left edge is its session's model (Opus, Sonnet, Haiku);
// - an in-progress card's border is its session's output: pulsing while
//   it prints (faster for more), faint for a while after, dark when quiet;
// - a Review card takes its reason's colour and says how long it has
//   waited, its glow wider the longer;
// - hatched: blocked, with what it waits on; faded: done;
// - greyed: its machine isn't connected.
//
// Sessions with no plan (and nothing to ask) are chips in a strip below.
// On a phone the columns stack at its width, Review first.

import type { FieldPane } from "./field";
import { REASON_COL } from "./model";
import { C, clamp, Flat, FONT, fmtDur, rateOf, reduce, type Rect } from "./flat";
import { boardOf, COLUMN_LABEL, COLUMNS, modelCol, type Board, type Card, type Column } from "./plan-model";

/** A column's width on a laptop, the gap between columns and between
 * cards, the header's height and a card's padding (world units). */
const CW = 184;
const GAP = 16;
const VGAP = 7;
const HEAD = 50;
const PAD = 9;
const STEP_LINE = 16;
const WHY_LINE = 13;
const SANS = (w: number, px: number) => `${w} ${px}px "Instrument Sans", system-ui, sans-serif`;
const ASK = REASON_COL.ask;
/** On a phone the columns stack, what waits on you first. */
const PHONE_ORDER: Column[] = ["review", "in_progress", "blocked", "pending", "done"];
/** A fit doesn't shrink the board below this to get it all in: it scrolls. */
const READABLE = 0.75;

interface Placed {
  card: Card;
  x: number;
  y: number;
  w: number;
  h: number;
  /** Precomputed, as the card's width allows. */
  head: string;
  lines: string[];
  why: string[];
  after: string | null;
  seed: number;
}

interface Chip {
  pane: string;
  x: number;
  y: number;
  w: number;
  h: number;
  text: string;
}

const rgba = (c: readonly number[], a: number) => `rgba(${c[0]},${c[1]},${c[2]},${a})`;

function seedOf(key: string): number {
  let h = 2166136261;
  for (let i = 0; i < key.length; i++) h = Math.imul(h ^ key.charCodeAt(i), 16777619);
  return ((h >>> 0) % 1000) / 1000;
}

/** How far a waiting card's glow reaches: wider the longer it waits (the
 * hive's spill, in card terms). */
const spill = (sec: number) => Math.min(48, 6 + 9 * Math.log2(1 + Math.max(0, sec) / 20));

export class PlanBoard extends Flat {
  private board: Board = { columns: { pending: [], in_progress: [], review: [], blocked: [], done: [] }, noPlan: [] };
  private placed: Placed[] = [];
  private heads: { col: Column; x: number; y: number; w: number }[] = [];
  private chips: Chip[] = [];
  private strip: { x: number; y: number } | null = null;
  private size = { w: 1, h: 1 };
  private sig = "";

  get clusters() {
    const n = (c: Column) => this.board.columns[c].length;
    return [
      ...COLUMNS.map((c) => ({ name: COLUMN_LABEL[c], n: n(c), need: c === "review" ? n(c) : 0 })),
      { name: "No plan", n: this.board.noPlan.length, need: 0 },
    ];
  }

  /** The cards as drawn, for tests: each with its pane, column and words. */
  cardsOf() {
    return this.placed.map((p) => ({ key: p.card.key, pane: p.card.pane, column: p.card.column, text: p.card.text, agent: p.card.agent, why: p.card.why ?? null, after: p.card.after ?? null, more: p.card.more ?? 0 }));
  }

  /** The sessions in the no-plan strip, by pane key. */
  noPlan() {
    return this.chips.map((c) => c.pane);
  }

  /** Where a card is on the page (tests click it). */
  screenOfCard(key: string): { x: number; y: number } | null {
    const p = this.placed.find((c) => c.card.key === key);
    if (!p) return null;
    const s = this.toScreen(p.x + p.w / 2, p.y + p.h / 2);
    const r = this.cv.getBoundingClientRect();
    return { x: s.x + r.left, y: s.y + r.top };
  }

  set(list: FieldPane[]) {
    super.set(list);
    // The steps change without the panes changing: lay out again then too.
    if (this.signature() !== this.sig) {
      this.layout();
      if (!this.userMoved) this.fit(false);
    }
  }

  private sessions() {
    return this.grouped()
      .flatMap((g) => g.ps)
      .filter((p) => p.plan)
      .map((p) => ({ key: p.key, plan: p.plan! }));
  }

  private signature(): string {
    return JSON.stringify(this.sessions().map((s) => [s.key, s.plan.steps, s.plan.waits, s.plan.title, s.plan.model, s.plan.agent])) + this.free().w;
  }

  private phone() {
    return this.free().w < 600;
  }

  protected layout() {
    this.sig = this.signature();
    this.board = boardOf(this.sessions());
    const cx = this.cx;
    const phone = this.phone();
    // On a phone the columns stack, as wide as the screen at scale 1.
    const cw = phone ? Math.max(220, this.free().w - 32) : CW;
    const wrap = (text: string, font: string, max: number, lines: number): string[] => {
      cx.font = font;
      const out: string[] = [];
      let line = "";
      const words = text.split(" ");
      for (let i = 0; i < words.length; i++) {
        const next = line ? `${line} ${words[i]}` : words[i];
        if (cx.measureText(next).width <= max || !line) {
          line = next;
          continue;
        }
        out.push(line);
        line = words[i];
        if (out.length === lines) break;
      }
      if (out.length < lines && line) out.push(line);
      // Cut the last line to fit, with an ellipsis if anything was left.
      const used = out.join(" ").length;
      if (out.length && (used < text.length || cx.measureText(out[out.length - 1]).width > max)) {
        let last = out[out.length - 1];
        while (last.length > 1 && cx.measureText(`${last}…`).width > max) last = last.slice(0, -1);
        out[out.length - 1] = used < text.length || last !== out[out.length - 1] ? `${last.trimEnd()}…` : last;
      }
      return out;
    };
    this.placed = [];
    this.heads = [];
    let x = 0;
    let y = 0;
    let bottom = 0;
    for (const col of phone ? PHONE_ORDER : COLUMNS) {
      if (phone) x = 0;
      else y = 0;
      this.heads.push({ col, x, y, w: cw });
      y += HEAD;
      const cards = this.board.columns[col];
      for (const card of cards) {
        const p = this.panes.get(card.pane);
        const inner = cw - PAD * 2 - 4;
        const step = wrap(card.text, SANS(600, 13), inner, card.column === "done" ? 2 : 3);
        const why = card.why ? wrap(card.why, FONT.mono(500, 10.5), inner, 2) : [];
        const after = card.after?.length
          ? wrap(`after ${card.after.join(", ")}`, FONT.mono(500, 10), inner, 1)[0]
          : card.more
            ? `and ${card.more} earlier step${card.more === 1 ? "" : "s"}`
            : null;
        const head = wrap(`%${p?.id ?? ""} ${p?.plan?.title ?? ""}`, FONT.mono(600, 9.5), inner - (card.column === "review" ? 34 : 0), 1)[0] ?? "";
        const h = PAD + 12 + 5 + step.length * STEP_LINE + (why.length ? 4 + why.length * WHY_LINE : 0) + (after ? WHY_LINE + 2 : 0) + 4 + 12 + PAD - 2;
        this.placed.push({ card, x, y, w: cw, h, head, lines: step, why, after, seed: seedOf(card.key) });
        y += h + VGAP;
      }
      if (!cards.length) y += 30;
      bottom = Math.max(bottom, y);
      if (phone) y += 18;
      else x += cw + GAP;
    }
    const width = phone ? cw : COLUMNS.length * cw + (COLUMNS.length - 1) * GAP;
    // The strip: sessions with no plan, as chips that wrap.
    this.chips = [];
    this.strip = null;
    let h = bottom;
    if (this.board.noPlan.length) {
      const sy = bottom + 26;
      this.strip = { x: 0, y: sy };
      cx.font = FONT.mono(600, 10.5);
      let cxp = 0;
      let cy = sy + 30;
      for (const key of this.board.noPlan) {
        const p = this.panes.get(key);
        const text = wrap(`%${p?.id ?? ""} ${p?.plan?.title ?? ""}`, FONT.mono(600, 10.5), 220, 1)[0] ?? "";
        const w = cx.measureText(text).width + 28;
        if (cxp > 0 && cxp + w > width) {
          cxp = 0;
          cy += 30;
        }
        this.chips.push({ pane: key, x: cxp, y: cy, w, h: 24, text });
        cxp += w + 8;
      }
      h = cy + 24;
    }
    this.size = { w: width, h: Math.max(h, HEAD + 40) };
  }

  /** A board reads from the top: fit its width, and its height only if
   * that doesn't make the words too small to read; then scroll. */
  protected fit(ease: boolean) {
    if (!this.W) return;
    const b = this.bounds();
    const f = this.free();
    const pad = this.phone() ? 16 : 24;
    const sw = (f.w - pad * 2) / Math.max(1, b.w);
    const sh = (f.h - 70 - pad) / Math.max(1, b.h);
    // A phone's stacked columns are laid out at its width: fit that.
    const s = clamp(this.phone() ? sw : Math.min(sw, Math.max(sh, Math.min(sw, READABLE)), 1.4), this.minS, this.fitMax);
    this.cam.ts = s;
    this.cam.tx = f.x + (f.w - b.w * s) / 2 - b.x * s;
    this.cam.ty = f.y + 6 - b.y * s;
    if (!ease || reduce) Object.assign(this.cam, { x: this.cam.tx, y: this.cam.ty, s: this.cam.ts });
  }

  protected bounds(): Rect {
    return { x: 0, y: 0, w: this.size.w, h: this.size.h };
  }

  protected posOf(key: string) {
    const c = this.placed.find((p) => p.card.pane === key) ?? this.chips.find((p) => p.pane === key);
    return c ? { x: c.x + c.w / 2, y: c.y + c.h / 2 } : null;
  }

  protected paneAt(sx: number, sy: number): FieldPane | null {
    const w = this.toWorld(sx, sy);
    const hit = [...this.placed, ...this.chips].find((c) => w.x >= c.x && w.x <= c.x + c.w && w.y >= c.y && w.y <= c.y + c.h);
    return hit ? (this.panes.get("card" in hit ? hit.card.pane : hit.pane) ?? null) : null;
  }

  protected draw(now: number) {
    const { cx, W, H } = this;
    const sec = now / 1000;
    cx.fillStyle = C.void;
    cx.fillRect(0, 0, W, H);
    this.world();

    // Columns: a faint lane each, its name and how many.
    cx.textBaseline = "alphabetic";
    for (const h of this.heads) {
      const n = this.board.columns[h.col].length;
      const review = h.col === "review" && n > 0;
      cx.fillStyle = "rgba(255,255,255,0.018)";
      const end = this.placed.filter((p) => p.card.column === h.col).reduce((m, p) => Math.max(m, p.y + p.h), h.y + HEAD + 24);
      cx.fillRect(h.x - 4, h.y + HEAD - 8, h.w + 8, end - (h.y + HEAD - 8) + 4);
      cx.font = FONT.display(800, 22);
      cx.fillStyle = review ? rgba(ASK, 1) : C.ink;
      const name = COLUMN_LABEL[h.col].toUpperCase();
      cx.fillText(name, h.x, h.y + 26);
      const nw = cx.measureText(name).width;
      cx.font = FONT.mono(700, 13);
      cx.fillStyle = review ? rgba(ASK, 0.9) : C.mute;
      cx.fillText(String(n), h.x + nw + 8, h.y + 26);
      cx.strokeStyle = C.line;
      cx.lineWidth = 1;
      cx.beginPath();
      cx.moveTo(h.x, h.y + 36);
      cx.lineTo(h.x + h.w, h.y + 36);
      cx.stroke();
      if (!n) {
        cx.font = FONT.mono(500, 10);
        cx.fillStyle = C.mute;
        cx.fillText("none", h.x, h.y + HEAD + 12);
      }
    }

    // What waits on you glows, wider the longer (added, so it tints).
    cx.globalCompositeOperation = "lighter";
    for (const c of this.placed) {
      if (c.card.column !== "review") continue;
      const p = this.panes.get(c.card.pane);
      if (!p || p.stale) continue;
      const col = p.att?.col ?? ASK;
      const wait = p.att?.since ? (now - p.att.since) / 1000 : 0;
      const pulse = reduce ? 1 : 0.8 + 0.2 * Math.sin(sec * 2.4 + c.seed * 40);
      const r = spill(wait);
      for (let k = 3; k >= 1; k--) {
        cx.fillStyle = rgba(col, (0.05 * pulse) / k);
        rounded(cx, c.x - (r * k) / 3, c.y - (r * k) / 3, c.w + (2 * r * k) / 3, c.h + (2 * r * k) / 3, 8 + (r * k) / 3);
        cx.fill();
      }
    }
    cx.globalCompositeOperation = "source-over";

    for (const c of this.placed) this.drawCard(c, now, sec);

    // Sessions with no plan.
    if (this.strip) {
      cx.font = FONT.display(800, 18);
      cx.fillStyle = C.ink;
      cx.fillText("NO PLAN", this.strip.x, this.strip.y + 16);
      const lw = cx.measureText("NO PLAN").width;
      cx.font = FONT.mono(500, 10);
      cx.fillStyle = C.mute;
      cx.fillText(`${this.chips.length} session${this.chips.length === 1 ? "" : "s"} without a task list`, this.strip.x + lw + 10, this.strip.y + 16);
      for (const ch of this.chips) {
        const p = this.panes.get(ch.pane);
        if (!p) continue;
        const mc = p.stale ? [70, 76, 88] : modelCol(p.plan?.model ?? null);
        rounded(cx, ch.x, ch.y, ch.w, ch.h, 12);
        cx.fillStyle = "#0c1019";
        cx.fill();
        cx.lineWidth = 1;
        const rate = p.stale ? 0 : rateOf(p.bps);
        cx.strokeStyle = rate > 0 ? rgba(mc, 0.7) : C.line;
        cx.stroke();
        cx.fillStyle = rgba(mc, p.stale ? 0.5 : 1);
        cx.beginPath();
        cx.arc(ch.x + 12, ch.y + ch.h / 2, 3.5, 0, Math.PI * 2);
        cx.fill();
        cx.font = FONT.mono(600, 10.5);
        cx.fillStyle = p.stale ? C.mute : C.ink;
        cx.textBaseline = "middle";
        cx.fillText(ch.text, ch.x + 21, ch.y + ch.h / 2 + 0.5);
        cx.textBaseline = "alphabetic";
      }
    }
    this.screen();
  }

  private drawCard(c: Placed, now: number, sec: number) {
    const cx = this.cx;
    const p = this.panes.get(c.card.pane);
    if (!p?.plan) return;
    const col = c.card.column;
    const stale = p.stale;
    const mc = stale ? [70, 76, 88] : modelCol(p.plan.model);
    const att = p.att?.col ?? ASK;
    cx.save();
    cx.globalAlpha = col === "done" ? 0.55 : stale ? 0.6 : 1;
    rounded(cx, c.x, c.y, c.w, c.h, 6);
    cx.fillStyle = "#0c1019";
    cx.fill();
    if (col === "review" && !stale) {
      cx.fillStyle = rgba(att, 0.13);
      cx.fill();
    }
    if (col === "blocked") {
      cx.save();
      rounded(cx, c.x, c.y, c.w, c.h, 6);
      cx.clip();
      cx.strokeStyle = "rgba(124,134,152,0.13)";
      cx.lineWidth = 1.2;
      cx.beginPath();
      for (let d = -c.h; d < c.w; d += 7) {
        cx.moveTo(c.x + d, c.y + c.h);
        cx.lineTo(c.x + d + c.h, c.y);
      }
      cx.stroke();
      cx.restore();
    }
    // The model's edge.
    cx.save();
    rounded(cx, c.x, c.y, c.w, c.h, 6);
    cx.clip();
    cx.fillStyle = rgba(mc, 0.95);
    cx.fillRect(c.x, c.y, 3.5, c.h);
    cx.restore();
    // The border: output while in progress, the reason's colour in review.
    rounded(cx, c.x + 0.5, c.y + 0.5, c.w - 1, c.h - 1, 6);
    if (col === "in_progress" && !stale) {
      const rate = rateOf(p.bps);
      const glow = p.lastOut ? Math.exp(-(now - p.lastOut) / 75_000) : 0;
      let a = 0.18 + glow * 0.3;
      let w = 1;
      if (rate > 0) {
        const pulse = reduce ? 1 : 0.6 + 0.4 * Math.sin(sec * (4 + rate * 14) + c.seed * 50);
        a = 0.45 + 0.55 * rate * pulse;
        w = 1.2 + rate * 1.6;
      }
      cx.strokeStyle = rgba(mc, a);
      cx.lineWidth = w;
    } else if (col === "review" && !stale) {
      cx.strokeStyle = rgba(att, 0.9);
      cx.lineWidth = 1.5;
    } else if (col === "blocked") {
      cx.setLineDash([4, 3]);
      cx.strokeStyle = "rgba(124,134,152,0.7)";
      cx.lineWidth = 1;
    } else {
      cx.strokeStyle = C.line;
      cx.lineWidth = 1;
    }
    cx.stroke();
    cx.setLineDash([]);

    const tx = c.x + PAD + 4;
    let y = c.y + PAD + 9;
    // The session, and how long it has waited on you.
    cx.font = FONT.mono(600, 9.5);
    cx.fillStyle = C.mute;
    cx.fillText(c.head, tx, y);
    if (col === "review" && p.att?.since) {
      const wait = (now - p.att.since) / 1000;
      cx.font = FONT.mono(700, 9.5);
      cx.fillStyle = rgba(att, 1);
      cx.textAlign = "right";
      cx.fillText(wait < 60 ? fmtDur(wait) : fmtDur(wait).replace(/\d+s$/, ""), c.x + c.w - PAD, y);
      cx.textAlign = "start";
    }
    y += 5;
    // The step.
    cx.font = SANS(600, 13);
    cx.fillStyle = col === "done" ? C.mute : C.ink;
    for (const l of c.lines) {
      y += STEP_LINE;
      cx.fillText(l, tx, y - 3);
    }
    // Why it waits; what it waits on.
    if (c.why.length) {
      y += 4;
      cx.font = FONT.mono(500, 10.5);
      cx.fillStyle = rgba(att, stale ? 0.5 : 1);
      for (const l of c.why) {
        y += WHY_LINE;
        cx.fillText(l, tx, y - 2);
      }
    }
    if (c.after) {
      y += WHY_LINE + 2;
      cx.font = FONT.mono(500, 10);
      cx.fillStyle = C.mute;
      cx.fillText(c.after, tx, y - 2);
    }
    // Who does it, on which model.
    y += 4 + 12;
    cx.font = FONT.mono(500, 9.5);
    cx.fillStyle = C.mute;
    const who = c.card.agent;
    cx.fillText(who, tx, y - 2);
    if (p.plan.model) {
      const ww = cx.measureText(`${who} · `).width;
      cx.fillText("·", tx + cx.measureText(`${who} `).width, y - 2);
      cx.fillStyle = rgba(mc, 1);
      cx.fillText(p.plan.model, tx + ww, y - 2);
    }
    cx.restore();
  }
}

function rounded(cx: CanvasRenderingContext2D, x: number, y: number, w: number, h: number, r: number) {
  cx.beginPath();
  cx.roundRect(x, y, w, h, Math.min(r, w / 2, h / 2));
}
