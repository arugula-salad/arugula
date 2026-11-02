// A View (#503): a layout you own whose leaves point at panes that live
// somewhere else, on any machine. Pure functions over its tree, so the page,
// the swarm's stage and the tests agree. docs/design/views.md says why it's
// shaped this way.

/** A leaf: `<machine>.<pane>`, control's device id or the host's name, then
 * the pane's number. The same form as `#swarm=<daemon>.N`. A leaf of a kind
 * this client doesn't know (one a newer client wrote) stays in the View as it
 * is, and draws as a placeholder: see `decode`. */
export type Address = string;

export type ViewNode = Address | ViewSplit;
export interface ViewSplit {
  /** `h`: side by side. `v`: one above the other. */
  d: "h" | "v";
  /** Each child's share of the split; they add up to 1. */
  s: number[];
  c: ViewNode[];
}

/** What's kept: one account pref per View (`view:<id>`). */
export interface ViewDoc {
  /** A name someone typed. Absent: it's named from its contents. */
  n?: string;
  t: ViewNode;
  /** When it was made (ms), for ordering. */
  at: number;
}

export interface Box {
  x: number;
  y: number;
  w: number;
  h: number;
}

export type Edge = "left" | "right" | "top" | "bottom" | "center";

/** A View holds this many leaves. More isn't legible, and this many always
 * fits a pref's 512 bytes with a full name. */
export const MAX_LEAVES = 9;
export const MAX_NAME = 40;
/** A pref's longest value, as JSON (control's prefs.rs). */
export const MAX_DOC_BYTES = 512;
/** The smallest share a divider can leave a child. */
export const MIN_SHARE = 0.12;

export function addressOf(machine: string, pane: number): Address {
  return `${machine}.${pane}`;
}

/** A machine's name can hold dots, so the pane is what follows the last. */
export function parseAddress(a: Address): { machine: string; pane: number } | null {
  const i = a.lastIndexOf(".");
  if (i <= 0) return null;
  const pane = Number(a.slice(i + 1));
  return Number.isInteger(pane) && pane >= 0 && a.slice(i + 1) !== "" ? { machine: a.slice(0, i), pane } : null;
}

export function leaves(n: ViewNode | null): Address[] {
  if (n === null) return [];
  return typeof n === "string" ? [n] : n.c.flatMap(leaves);
}

const rounded = (v: number) => Math.round(v * 1000) / 1000;

/** Each share to three places, so a pref stays short, still adding up to 1. */
function shares(values: number[]): number[] {
  const sum = values.reduce((a, b) => a + b, 0) || 1;
  const out = values.map((v) => rounded(v / sum));
  out[out.length - 1] = rounded(1 - out.slice(0, -1).reduce((a, b) => a + b, 0));
  return out;
}

/** `add` beside the leaf `at`, taking half of it. A center drop splits along
 * the leaf's long side when the caller knows it (`bestEdge`); here, sideways. */
export function insert(n: ViewNode | null, at: Address | null, edge: Edge, add: Address): ViewNode {
  if (n === null || at === null) return n === null ? add : insertAtRoot(n, edge, add);
  if (typeof n === "string") {
    if (n !== at) return n;
    const side = edge === "center" ? "right" : edge;
    const d = side === "left" || side === "right" ? "h" : "v";
    return { d, s: [0.5, 0.5], c: side === "left" || side === "top" ? [add, n] : [n, add] };
  }
  return { ...n, c: n.c.map((c) => insert(c, at, edge, add)) };
}

function insertAtRoot(n: ViewNode, edge: Edge, add: Address): ViewNode {
  const side = edge === "center" ? "right" : edge;
  const d = side === "left" || side === "right" ? "h" : "v";
  return { d, s: [0.5, 0.5], c: side === "left" || side === "top" ? [add, n] : [n, add] };
}

/** Take a leaf out: its split closes up, and a split left with one child
 * becomes that child. Null when nothing's left. */
export function remove(n: ViewNode | null, a: Address): ViewNode | null {
  if (n === null) return null;
  if (typeof n === "string") return n === a ? null : n;
  const kept: ViewNode[] = [];
  const sizes: number[] = [];
  n.c.forEach((c, i) => {
    const next = remove(c, a);
    if (next !== null) {
      kept.push(next);
      sizes.push(n.s[i]);
    }
  });
  if (kept.length === 0) return null;
  if (kept.length === 1) return kept[0];
  return { ...n, c: kept, s: kept.length === n.c.length ? n.s : shares(sizes) };
}

/** A divider between two children of a split. */
export interface Divider {
  /** The child indexes down to the split. */
  path: number[];
  /** Between child `i` and `i + 1`. */
  i: number;
  d: "h" | "v";
  /** Where it is along the split's direction, in the box's coordinates. */
  at: number;
  /** The box of the split. */
  box: Box;
}

/** Where every leaf and divider goes in `box`. */
export function arrange(n: ViewNode | null, box: Box): { leaves: Map<Address, Box>; dividers: Divider[] } {
  const out = new Map<Address, Box>();
  const dividers: Divider[] = [];
  const walk = (node: ViewNode, b: Box, path: number[]) => {
    if (typeof node === "string") {
      out.set(node, b);
      return;
    }
    let at = node.d === "h" ? b.x : b.y;
    const span = node.d === "h" ? b.w : b.h;
    node.c.forEach((c, i) => {
      const size = node.s[i] * span;
      walk(c, node.d === "h" ? { x: at, y: b.y, w: size, h: b.h } : { x: b.x, y: at, w: b.w, h: size }, [...path, i]);
      at += size;
      if (i < node.c.length - 1) dividers.push({ path, i, d: node.d, at, box: b });
    });
  };
  if (n !== null) walk(n, box, []);
  return { leaves: out, dividers };
}

/** Move a divider by `delta` of its split, leaving each side `MIN_SHARE`. */
export function resize(n: ViewNode, path: number[], i: number, delta: number): ViewNode {
  if (typeof n === "string") return n;
  if (path.length > 0) {
    const [head, ...rest] = path;
    return { ...n, c: n.c.map((c, k) => (k === head ? resize(c, rest, i, delta) : c)) };
  }
  const total = n.s[i] + n.s[i + 1];
  const a = Math.min(total - MIN_SHARE, Math.max(MIN_SHARE, n.s[i] + delta));
  const s = [...n.s];
  s[i] = rounded(a);
  s[i + 1] = rounded(total - a);
  return { ...n, s };
}

/** The biggest leaf, and the edge along its long side: where a fling or a
 * key lands, with no aiming. */
export function bestSpot(n: ViewNode | null, box: Box): { at: Address; edge: Edge } | null {
  let best: { at: Address; edge: Edge } | null = null;
  let area = -1;
  for (const [at, b] of arrange(n, box).leaves) {
    if (b.w * b.h > area) {
      area = b.w * b.h;
      best = { at, edge: bestEdge(b) };
    }
  }
  return best;
}

/** A wide leaf splits sideways, a tall one downward. */
export const bestEdge = (b: Box): Edge => (b.w >= b.h * 1.2 ? "right" : "bottom");

/** Where a pointer at (x, y) would land: the nearest edge of the leaf under
 * it, or its middle (the same zones as a tab's panes, ui/drag.ts). */
export function zoneAt(n: ViewNode | null, box: Box, x: number, y: number, edgeZone: number): { at: Address | null; edge: Edge; box: Box } | null {
  if (x < box.x || x > box.x + box.w || y < box.y || y > box.y + box.h) return null;
  if (n === null) return { at: null, edge: "center", box };
  for (const [at, b] of arrange(n, box).leaves) {
    if (x < b.x || x > b.x + b.w || y < b.y || y > b.y + b.h) continue;
    const fx = (x - b.x) / b.w;
    const fy = (y - b.y) / b.h;
    const edges: [Edge, number][] = [
      ["left", fx],
      ["right", 1 - fx],
      ["top", fy],
      ["bottom", 1 - fy],
    ];
    const [edge, dist] = edges.reduce((a, c) => (c[1] < a[1] ? c : a));
    return { at, edge: dist < edgeZone ? edge : "center", box: b };
  }
  return null;
}

/** The part of a leaf a drop in that zone would take. */
export function zoneBox(edge: Edge, b: Box): Box {
  switch (edge) {
    case "left":
      return { ...b, w: b.w / 2 };
    case "right":
      return { ...b, x: b.x + b.w / 2, w: b.w / 2 };
    case "top":
      return { ...b, h: b.h / 2 };
    case "bottom":
      return { ...b, y: b.y + b.h / 2, h: b.h / 2 };
    default:
      return b;
  }
}

/** A split as it's kept: `["h", [0.5, 0.5], a, b]`. Written out, nine leaves
 * and a full name would pass a pref's 512 bytes. */
type WireNode = Address | [ViewSplit["d"], number[], ...WireNode[]];

const toWire = (n: ViewNode): WireNode => (typeof n === "string" ? n : [n.d, n.s, ...n.c.map(toWire)]);

/** What's kept for a View. */
export function encode(doc: ViewDoc): { n?: string; t: WireNode; at: number } {
  return { ...(doc.n ? { n: doc.n } : {}), t: toWire(doc.t), at: doc.at };
}

/** Whether a View may hold this: few enough leaves, and short enough to keep
 * as a pref. */
export function fits(doc: ViewDoc): boolean {
  return leaves(doc.t).length <= MAX_LEAVES && JSON.stringify(encode(doc)).length <= MAX_DOC_BYTES;
}

function fromWire(n: unknown): ViewNode | null {
  // Any leaf a newer client may have written: kept, and saved back unchanged.
  if (typeof n === "string") return n ? n : null;
  if (!Array.isArray(n)) return null;
  const [d, s, ...rest] = n as [unknown, unknown, ...unknown[]];
  if ((d !== "h" && d !== "v") || !Array.isArray(s) || rest.length < 2 || s.length !== rest.length) return null;
  if (!s.every((v) => typeof v === "number" && v > 0)) return null;
  const c = rest.map(fromWire);
  return c.every((x) => x !== null) ? { d, s: s as number[], c: c as ViewNode[] } : null;
}

/** A kept value, or null when it isn't a View (a pref can hold anything an
 * older page wrote). A leaf it can't read doesn't make it "not a View". */
export function decode(v: unknown): ViewDoc | null {
  if (!v || typeof v !== "object") return null;
  const { n, t, at } = v as { n?: unknown; t?: unknown; at?: unknown };
  const tree = fromWire(t);
  if (tree === null || typeof at !== "number") return null;
  return { t: tree, at, ...(typeof n === "string" && n ? { n: n.slice(0, MAX_NAME) } : {}) };
}

/** A View's name: the one typed, else from what's in it. */
export function viewName(doc: Pick<ViewDoc, "n" | "t"> | null, title: (a: Address) => string): string {
  if (doc?.n) return doc.n;
  const ls = leaves(doc?.t ?? null);
  if (ls.length === 0) return "New view";
  return ls.length === 1 ? title(ls[0]) : `${title(ls[0])} + ${ls.length - 1} more`;
}

/** What a terminal leaf asks of its tab's size (#473, DECISIONS.md "Size
 * arbitration"). A tab has one size until #477, so two leaves of one tab
 * leave it alone. */
export type SizeAsk = "keep" | "claim" | "ask" | "none";

export function sizeAsk(s: {
  /** Who holds the tab's size; null: nobody. */
  owner: number | null;
  /** This link's client id on that machine. */
  me: number;
  /** That client id belongs to this same page's main client. */
  ownPage: boolean;
  /** That client is still connected. */
  present: boolean;
  /** How many leaves of this View show that tab. */
  sharers: number;
  /** A viewer has no say in a tab's size. */
  viewer: boolean;
}): SizeAsk {
  if (s.viewer || s.sharers > 1) return "none";
  if (s.owner === s.me) return "keep";
  if (s.owner === null || !s.present) return "claim";
  return s.ownPage ? "ask" : "none";
}

/** What a leaf of a kind this client doesn't know is called, and says. */
export const UNKNOWN_LEAF = "Unknown block";
export const UNKNOWN_LEAF_NOTE = "This version of Arugula can't show this block. Update to see it.";

/** What a leaf shows instead of its pane, when it can't show it. */
export function placeholder(s: {
  machine: string;
  pane: number;
  known: boolean;
  connected: boolean;
  hasState: boolean;
  hasPane: boolean;
  /** The daemon lists roles: this person isn't its owner. */
  shared: boolean;
  /** A pane not yet seen gets this long before it counts as closed. */
  graceOver: boolean;
}): string | null {
  if (!s.known) return "This pane's machine isn't in your list.";
  if (!s.connected) return `${s.machine} is unreachable, reconnecting…`;
  if (!s.hasState || s.hasPane) return null;
  if (s.shared) return `%${s.pane} on ${s.machine} isn't shared with you.`;
  return s.graceOver ? `%${s.pane} has closed on ${s.machine}.` : null;
}
