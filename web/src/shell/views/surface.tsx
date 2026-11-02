// The live blocks of a View, and everything that moves them (#503). A block is
// one element from the moment it's picked up: the card under the pointer, the
// body in flight and the leaf it lands in, still streaming the whole way. So
// they're all fixed to the window and placed by springs, and the layout tree
// only says where each is heading. docs/design/views.md, and the numbers are
// in ./motion.

import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "preact/hooks";
import type { RefObject } from "preact";
import type { Client } from "../client";
import { paneIds } from "../client";
import type { Cell } from "../ui/cells";
import { PaneSlot } from "../ui/panes/pane-slot";
import { paneGlyph, paneStatus, PaneTypeIcon } from "../ui/shell/pane-header";
import { MoreIcon } from "../ui/shell/icons";
import { openMenu, type MenuItem } from "../../shared/ui/menu";
import { kindOf, KINDS } from "../swarm/model";
import type { FleetPane } from "../fleet";
import { hostOf, viewLinks } from "./links";
import {
  arrange,
  bestEdge,
  insert,
  leaves,
  parseAddress,
  placeholder,
  remove,
  resize,
  sizeAsk,
  zoneAt,
  zoneBox,
  MAX_LEAVES,
  UNKNOWN_LEAF,
  UNKNOWN_LEAF_NOTE,
  type Address,
  type Box,
  type Divider,
  type Edge,
  type ViewNode,
  bestSpot,
} from "./model";
import { FONT_SIZE } from "../terminal-view";
import { MOTION, reducedMotion, spring } from "./motion";
import { getView, saveTree, subscribeViews } from "./store";

/** A pane not yet seen gets this long before its leaf calls it closed: its
 * machine's state can arrive after the leaf (the same grace as a remote pane). */
const GONE_GRACE_MS = 5000;
/** A size asked of a tab waits for a divider or a window to stop moving: a
 * terminal's reflow is the costly part. */
const SIZE_SETTLE_MS = 160;
/** A leaf's header, reserved above its pane. */
const HEAD_H = 26;
/** The smallest a terminal is drawn on a phone: text at 10px. */
const READABLE = 10 / FONT_SIZE;
const CARD = { w: 320, h: 200 };
const TILE = { w: 24, h: 15 };

export interface SurfaceApi {
  /** Pick these panes up: they follow the pointer until it's released. */
  carry(list: Address[], at: { clientX: number; clientY: number }, from?: (a: Address) => Box | null): void;
  /** Send these into the View with no aiming: the keyboard's and a menu's way. */
  send(list: Address[], from?: (a: Address) => Box | null): void;
}

interface Body {
  a: Address;
  el: HTMLElement | null;
  x: number;
  y: number;
  w: number;
  h: number;
  vx: number;
  vy: number;
  vw: number;
  vh: number;
  mode: "placed" | "carried" | "home";
  /** When its flight starts (ms): the pull ramps up from then. */
  flew: number;
  off: { x: number; y: number };
  lag: number;
  tearing: boolean;
}

interface Props {
  /** The View. */
  id: string;
  /** The element its leaves are laid out in. */
  area: RefObject<HTMLElement | null>;
  /** This page's main client, for handing a tab's size over. */
  main: Client;
  cell: Cell;
  phone: boolean;
  /** Leaves ask their tabs for the size (the View's page). The swarm's stage
   * is a preview, and scales them. */
  sizing: boolean;
  /** Where a pane goes home to, on the screen. */
  homeOf: (a: Address) => Box | null;
  /** The surface is ready for gestures. */
  onReady?: (api: SurfaceApi | null) => void;
  /** A gesture started or stopped, for the stage to wake. */
  onCarrying?: (on: boolean) => void;
  /** The View's tree changed here. */
  onChange?: (t: ViewNode | null) => void;
}

export function ViewSurface({ id, area, main, cell, phone, sizing, homeOf, onReady, onCarrying, onChange }: Props) {
  const [tree, setTree] = useState<ViewNode | null>(() => getView(id)?.t ?? null);
  const treeRef = useRef(tree);
  treeRef.current = tree;
  /** What's mounted: the tree's leaves, and what's in the air. */
  const [flying, setFlying] = useState<Address[]>([]);
  const [active, setActive] = useState<Address | null>(null);
  const [zone, setZone] = useState<Box | null>(null);
  const [soloAt, setSoloAt] = useState(0);
  const [, setAreaMoved] = useState(0);
  const bodies = useRef(new Map<Address, Body>());
  const pending = useRef(new Map<Address, Box>());
  const pointer = useRef({ x: 0, y: 0, vx: 0, vy: 0, t: 0 });
  const carrying = useRef<{ list: Address[]; fromLeaf: boolean; before: ViewNode | null } | null>(null);
  const dragging = useRef(false);
  const raf = useRef(0);
  const trails = useRef<HTMLCanvasElement>(null);

  const areaBox = (): Box => {
    const r = area.current?.getBoundingClientRect();
    return r ? { x: r.left, y: r.top, w: r.width, h: r.height } : { x: 0, y: 0, w: 0, h: 0 };
  };

  // Another tab or device changed it, and nothing's in hand here.
  useEffect(() => {
    const sync = () => {
      if (carrying.current || dragging.current) return;
      const t = getView(id)?.t ?? null;
      if (JSON.stringify(t) !== JSON.stringify(treeRef.current)) setTree(t);
    };
    const off = subscribeViews(sync);
    // The prefs may have loaded between the first render and now, on a page
    // opened at a View.
    sync();
    return off;
  }, [id]);

  const commit = (t: ViewNode | null): boolean => {
    if (!saveTree(id, t)) return false;
    setTree(t);
    onChange?.(t);
    return true;
  };

  // The frame: every body one spring step toward where it's heading. It runs
  // only while something moves.
  const wake = () => {
    if (!raf.current) raf.current = requestAnimationFrame(frame);
  };
  const last = useRef(0);
  const frame = (now: number) => {
    raf.current = 0;
    const dt = Math.min(0.033, last.current ? (now - last.current) / 1000 : 0.016);
    last.current = now;
    const reduced = reducedMotion();
    const box = areaBox();
    const placed = arrange(treeRef.current, box).leaves;
    const solo = phone ? leaves(treeRef.current)[soloRef.current] : undefined;
    const p = pointer.current;
    const cv = trails.current;
    const cx = cv?.getContext("2d") ?? null;
    if (cv && cx && !reduced) {
      const d = devicePixelRatio || 1;
      if (cv.width !== Math.floor(innerWidth * d)) {
        cv.width = Math.floor(innerWidth * d);
        cv.height = Math.floor(innerHeight * d);
      }
      cx.setTransform(d, 0, 0, d, 0, 0);
      cx.globalCompositeOperation = "destination-out";
      cx.fillStyle = "rgba(0,0,0,.16)";
      cx.fillRect(0, 0, innerWidth, innerHeight);
      cx.globalCompositeOperation = "source-over";
    }
    let moving = false;
    const gone: Address[] = [];
    for (const b of bodies.current.values()) {
      if (!b.el) continue;
      let t: Box | null = null;
      let k = MOTION.stiffness;
      let c = MOTION.damping;
      let air = false;
      if (b.mode === "carried") {
        const size = b.tearing ? TILE : phone ? { w: Math.min(CARD.w, innerWidth * 0.7), h: CARD.h * 0.8 } : CARD;
        t = { x: p.x + b.off.x - size.w * 0.3, y: p.y + b.off.y - 12, ...size };
        k = MOTION.stiffness * 2.2 * b.lag;
        c = 2 * Math.sqrt(k) * 0.82;
        air = true;
        moving = true;
      } else {
        t = b.mode === "home" ? (homeOf(b.a) ?? { x: box.x + box.w / 2, y: box.y + box.h + 40, ...TILE }) : (placed.get(b.a) ?? null);
        if (b.mode === "placed" && solo !== undefined && solo !== b.a) {
          b.el.style.display = "none";
          continue;
        }
        if (b.mode === "placed" && solo === b.a) t = box;
        if (!t) {
          b.el.style.display = "none";
          continue;
        }
        // In flight the pull ramps up, so a flick keeps its momentum first.
        const age = now - b.flew;
        if (age < 0) {
          t = { x: b.x, y: b.y, w: b.w, h: b.h };
          k = 30;
          c = 5;
          air = true;
        } else if (age < MOTION.flightMs) {
          const ramp = age / MOTION.flightMs;
          k = MOTION.stiffness * (0.12 + 0.88 * ramp * ramp);
          c = MOTION.damping * (0.25 + 0.75 * ramp * ramp);
          air = true;
        }
      }
      b.el.style.display = "";
      const px = b.x;
      const py = b.y;
      if (reduced && b.mode !== "carried") {
        Object.assign(b, { x: t.x, y: t.y, w: t.w, h: t.h, vx: 0, vy: 0, vw: 0, vh: 0 });
      } else {
        const o = b as unknown as Record<string, number>;
        spring(o, "x", "vx", t.x, k, c, dt);
        spring(o, "y", "vy", t.y, k, c, dt);
        spring(o, "w", "vw", t.w, Math.max(k, 240), Math.max(c, 24), dt);
        spring(o, "h", "vh", t.h, Math.max(k, 240), Math.max(c, 24), dt);
      }
      const still = b.mode !== "carried" && now >= b.flew && Math.abs(b.x - t.x) + Math.abs(b.y - t.y) + Math.abs(b.w - t.w) + Math.abs(b.h - t.h) < 0.6 && Math.abs(b.vx) + Math.abs(b.vy) + Math.abs(b.vw) + Math.abs(b.vh) < 8;
      if (still) {
        Object.assign(b, { x: t.x, y: t.y, w: t.w, h: t.h, vx: 0, vy: 0, vw: 0, vh: 0 });
        if (b.mode === "home") gone.push(b.a);
      } else moving = true;
      const w = Math.max(8, b.w);
      const h = Math.max(6, b.h);
      b.el.style.width = `${w}px`;
      b.el.style.height = `${h}px`;
      // A leaf at rest sits on whole pixels, or its text blurs.
      const [tx, ty] = still ? [Math.round(b.x), Math.round(b.y)] : [b.x, b.y];
      b.el.style.transform = `translate(${tx}px, ${ty}px)`;
      b.el.classList.toggle("flying", b.mode !== "placed" || air);
      const speed = Math.hypot(b.x - px, b.y - py);
      if (cx && !reduced && MOTION.trail > 0 && speed > 3 && (air || b.mode !== "placed")) {
        cx.strokeStyle = `rgba(${b.el.dataset.kindRgb ?? "185,140,255"},${0.6 * MOTION.trail})`;
        cx.lineWidth = 1 + Math.min(5, speed / 7) * MOTION.trail;
        cx.lineCap = "round";
        cx.beginPath();
        cx.moveTo(px + w / 2, py + h / 2);
        cx.lineTo(b.x + w / 2, b.y + h / 2);
        cx.stroke();
      }
    }
    if (gone.length) {
      for (const a of gone) bodies.current.delete(a);
      setFlying((f) => f.filter((a) => !gone.includes(a)));
    }
    if (moving) wake();
    else {
      last.current = 0;
      // The trails fade out with the last of the motion.
      cx?.clearRect(0, 0, innerWidth, innerHeight);
    }
  };
  const soloRef = useRef(0);
  soloRef.current = soloAt;

  /** Make a body for a pane about to mount, starting from `from`. */
  const spawn = (a: Address, from: Box | null): Body => {
    let b = bodies.current.get(a);
    if (b) return b;
    const start = from ?? pending.current.get(a) ?? arrange(treeRef.current, areaBox()).leaves.get(a) ?? { x: pointer.current.x, y: pointer.current.y, ...TILE };
    b = { a, el: null, x: start.x, y: start.y, w: start.w, h: start.h, vx: 0, vy: 0, vw: 0, vh: 0, mode: "placed", flew: 0, off: { x: 0, y: 0 }, lag: 1, tearing: false };
    bodies.current.set(a, b);
    return b;
  };

  const elRef = (a: Address) => (el: HTMLElement | null) => {
    const b = spawn(a, null);
    b.el = el;
    if (el) wake();
  };

  // ---- carrying

  const move = (e: PointerEvent) => {
    const p = pointer.current;
    const now = performance.now();
    const dt = Math.max(1, now - p.t) / 1000;
    p.vx = p.vx * 0.72 + ((e.clientX - p.x) / dt) * 0.28;
    p.vy = p.vy * 0.72 + ((e.clientY - p.y) / dt) * 0.28;
    p.x = e.clientX;
    p.y = e.clientY;
    p.t = now;
    const box = areaBox();
    const z = zoneAt(treeRef.current, box, p.x, p.y, MOTION.edge);
    setZone(z ? zoneBox(z.edge, z.box) : null);
    const c = carrying.current;
    if (c?.fromLeaf) {
      const far = Math.hypot(Math.max(box.x - p.x, 0, p.x - (box.x + box.w)), Math.max(box.y - p.y, 0, p.y - (box.y + box.h)));
      for (const a of c.list) {
        const b = bodies.current.get(a);
        if (b) b.tearing = far > MOTION.tear;
      }
    }
    wake();
  };

  const endGesture = () => {
    removeEventListener("pointermove", move, true);
    removeEventListener("pointerup", up, true);
    removeEventListener("pointercancel", cancel, true);
    removeEventListener("keydown", escape, true);
    document.body.classList.remove("view-carrying");
    setZone(null);
    onCarrying?.(false);
    const c = carrying.current;
    carrying.current = null;
    return c;
  };

  const launch = (b: Body, i: number) => {
    const p = pointer.current;
    b.vx = p.vx;
    b.vy = p.vy;
    b.flew = performance.now() + i * MOTION.stagger;
    b.tearing = false;
  };

  /** Into the tree at a spot, else where there's most room. False: no room. */
  const place = (t: ViewNode | null, a: Address, spot: { at: Address | null; edge: Edge } | null): ViewNode | null => {
    const without = remove(t, a);
    if (!leaves(without).includes(a) && leaves(without).length >= MAX_LEAVES) return null;
    const where = spot ?? bestSpot(without, areaBox());
    return insert(without, where?.at ?? null, where?.edge ?? "center", a);
  };

  const land = (list: Address[], first: { at: Address | null; edge: Edge } | null) => {
    let t = treeRef.current;
    let refused = false;
    list.forEach((a, i) => {
      const b = bodies.current.get(a);
      if (!b) return;
      launch(b, i);
      const next = place(t, a, i === 0 ? first : null);
      if (next === null) {
        refused = true;
        b.mode = "home";
      } else {
        t = next;
        b.mode = "placed";
      }
    });
    if (!commit(t)) {
      refused = true;
      for (const a of list) {
        const b = bodies.current.get(a);
        if (b) b.mode = "home";
      }
    }
    if (refused) main.toast(`A view holds ${MAX_LEAVES} blocks.`);
    setFlying((f) => f.filter((a) => !list.includes(a) || bodies.current.get(a)?.mode === "home"));
    wake();
  };

  const up = (e: PointerEvent) => {
    move(e);
    const p = pointer.current;
    const c = endGesture();
    if (!c) return;
    const box = areaBox();
    const z = zoneAt(treeRef.current, box, p.x, p.y, MOTION.edge);
    if (z) {
      const edge = z.edge === "center" && z.at ? bestEdge(z.box) : z.edge;
      return land(c.list, { at: z.at, edge });
    }
    // Flicked at the View: it finds its own way there.
    const speed = Math.hypot(p.vx, p.vy);
    const to = { x: box.x + box.w / 2 - p.x, y: box.y + box.h / 2 - p.y };
    const cosine = (to.x * p.vx + to.y * p.vy) / ((Math.hypot(to.x, to.y) || 1) * (speed || 1));
    if (speed > MOTION.flingSpeed && cosine > 0.7) return land(c.list, null);
    // Dropped short of leaving: a leaf goes back where it was. Past the tear
    // distance, or from the swarm, it goes home.
    const torn = c.list.some((a) => bodies.current.get(a)?.tearing);
    if (c.fromLeaf && !torn) {
      setTree(c.before);
      for (const a of c.list) {
        const b = bodies.current.get(a);
        if (b) {
          launch(b, 0);
          b.mode = "placed";
        }
      }
      setFlying((f) => f.filter((a) => !c.list.includes(a)));
      return wake();
    }
    c.list.forEach((a, i) => {
      const b = bodies.current.get(a);
      if (b) {
        launch(b, i);
        b.mode = "home";
      }
    });
    if (c.fromLeaf) commit(treeRef.current);
    wake();
  };

  const cancel = () => {
    const c = endGesture();
    if (!c) return;
    if (c.fromLeaf) setTree(c.before);
    for (const a of c.list) {
      const b = bodies.current.get(a);
      if (b) b.mode = c.fromLeaf ? "placed" : "home";
    }
    setFlying((f) => (c.fromLeaf ? f.filter((a) => !c.list.includes(a)) : f));
    wake();
  };

  const escape = (e: KeyboardEvent) => {
    if (e.key !== "Escape" || !carrying.current) return;
    e.preventDefault();
    e.stopPropagation();
    cancel();
  };

  const startCarry = (list: Address[], at: { clientX: number; clientY: number }, fromLeaf: boolean, from?: (a: Address) => Box | null) => {
    if (carrying.current) return;
    const before = treeRef.current;
    const p = pointer.current;
    Object.assign(p, { x: at.clientX, y: at.clientY, vx: 0, vy: 0, t: performance.now() });
    list.forEach((a, i) => {
      const box = from?.(a);
      if (box) pending.current.set(a, box);
      const b = spawn(a, box ?? null);
      b.mode = "carried";
      b.off = { x: i * 18, y: i * 14 };
      b.lag = Math.pow(MOTION.flockLag, Math.min(i, 3));
      b.tearing = false;
    });
    if (fromLeaf) {
      let t = before;
      for (const a of list) t = remove(t, a);
      setTree(t);
    }
    setFlying((f) => [...new Set([...f, ...list])]);
    carrying.current = { list, fromLeaf, before };
    document.body.classList.add("view-carrying");
    onCarrying?.(true);
    addEventListener("pointermove", move, true);
    addEventListener("pointerup", up, true);
    addEventListener("pointercancel", cancel, true);
    addEventListener("keydown", escape, true);
    wake();
  };

  useEffect(() => {
    onReady?.({
      carry: (list, at, from) => startCarry(list, at, false, from),
      send: (list, from) => {
        for (const a of list) spawn(a, from?.(a) ?? { x: areaBox().x, y: areaBox().y + areaBox().h / 2, ...TILE });
        setFlying((f) => [...new Set([...f, ...list])]);
        land(list, null);
      },
    });
    return () => onReady?.(null);
  });

  // The window or the area moved: every leaf follows.
  useEffect(() => {
    const el = area.current;
    if (!el) return;
    const moved = () => {
      wake();
      setAreaMoved((n) => n + 1);
    };
    const ro = new ResizeObserver(moved);
    ro.observe(el);
    addEventListener("resize", moved);
    return () => {
      ro.disconnect();
      removeEventListener("resize", moved);
    };
  }, []);
  useLayoutEffect(wake);
  useEffect(
    () => () => {
      cancelAnimationFrame(raf.current);
      raf.current = 0;
      if (carrying.current) endGesture();
    },
    [],
  );

  // ---- dividers

  const startDivider = (e: PointerEvent, d: Divider) => {
    e.preventDefault();
    e.stopPropagation();
    dragging.current = true;
    const start = d.d === "h" ? e.clientX : e.clientY;
    const span = d.d === "h" ? d.box.w : d.box.h;
    const base = treeRef.current;
    if (!base) return;
    const m = (ev: PointerEvent) => {
      setTree(resize(base, d.path, d.i, ((d.d === "h" ? ev.clientX : ev.clientY) - start) / (span || 1)));
    };
    const u = () => {
      removeEventListener("pointermove", m, true);
      removeEventListener("pointerup", u, true);
      removeEventListener("pointercancel", u, true);
      dragging.current = false;
      commit(treeRef.current);
    };
    addEventListener("pointermove", m, true);
    addEventListener("pointerup", u, true);
    addEventListener("pointercancel", u, true);
  };

  // ---- what's drawn

  const placedList = leaves(tree);
  const all = [...new Set([...placedList, ...flying])];
  const removeLeaf = (a: Address) => {
    const b = bodies.current.get(a);
    if (b) {
      b.mode = "home";
      b.flew = performance.now();
    }
    setFlying((f) => [...new Set([...f, a])]);
    commit(remove(treeRef.current, a));
    wake();
  };

  const box = areaBox();
  const { leaves: places, dividers: laidOut } = arrange(tree, box);
  const dividers = phone ? [] : laidOut;
  const solo = phone ? placedList[Math.min(soloAt, Math.max(0, placedList.length - 1))] : undefined;

  // Keys: reading order, as in a tab.
  useEffect(() => {
    if (!sizing) return;
    const key = (e: KeyboardEvent) => {
      if (!((e.ctrlKey || e.metaKey) && e.shiftKey) || !/^Digit[1-9]$/.test(e.code)) return;
      const a = placedList[Number(e.code.slice(5)) - 1];
      const at = a ? parseAddress(a) : null;
      if (!a || !at) return;
      e.preventDefault();
      e.stopPropagation();
      setActive(a);
      viewLinks.client(at.machine)?.viewOf(at.pane)?.focus();
    };
    addEventListener("keydown", key, true);
    return () => removeEventListener("keydown", key, true);
  });

  // Who's being looked at: each machine hears about its own leaf only.
  useEffect(() => {
    const report = () => {
      const looking = document.visibilityState === "visible" && (phone || document.hasFocus());
      const seen = new Set<string>();
      for (const a of leaves(treeRef.current)) {
        const at = parseAddress(a);
        if (!at || seen.has(at.machine)) continue;
        seen.add(at.machine);
        const on = active && parseAddress(active)?.machine === at.machine ? parseAddress(active)!.pane : null;
        viewLinks.client(at.machine)?.focusPane(looking ? on : null);
      }
    };
    report();
    addEventListener("focus", report);
    addEventListener("blur", report);
    document.addEventListener("visibilitychange", report);
    return () => {
      removeEventListener("focus", report);
      removeEventListener("blur", report);
      document.removeEventListener("visibilitychange", report);
    };
  }, [active, tree, phone]);

  const shared: Shared = {
    main,
    cell,
    phone,
    sizing,
    addresses: placedList,
    active,
    setActive,
    remove: removeLeaf,
    grabLeaf: (a, e) => startCarry([a], e, true),
    elRef,
    wake,
  };

  return (
    <>
      <canvas class="view-trails" ref={trails} aria-hidden="true" />
      {all.map((a, i) => (
        <Leaf key={a} a={a} n={placedList.indexOf(a) + 1} s={shared} place={phone && a === solo ? box : places.get(a)} hidden={solo !== undefined && a !== solo && placedList.includes(a)} index={i} />
      ))}
      {dividers.map((d) => (
        <div
          key={`${d.path.join(".")}/${d.i}`}
          class={`view-divider ${d.d}`}
          role="separator"
          aria-orientation={d.d === "h" ? "vertical" : "horizontal"}
          style={d.d === "h" ? { left: `${d.at - 4}px`, top: `${d.box.y}px`, width: "8px", height: `${d.box.h}px` } : { left: `${d.box.x}px`, top: `${d.at - 4}px`, width: `${d.box.w}px`, height: "8px" }}
          onPointerDown={(e) => startDivider(e, d)}
        />
      ))}
      {zone && <div class="view-zone" data-view-zone style={{ left: `${zone.x}px`, top: `${zone.y}px`, width: `${zone.w}px`, height: `${zone.h}px` }} />}
      {phone && placedList.length > 1 && (
        <div class="view-solo-bar" data-view-solo>
          <button aria-label="Previous block" onClick={() => setSoloAt((placedList.indexOf(solo!) - 1 + placedList.length) % placedList.length)}>
            ‹
          </button>
          <span>
            {placedList.indexOf(solo!) + 1} of {placedList.length}
          </span>
          <button aria-label="Next block" onClick={() => setSoloAt((placedList.indexOf(solo!) + 1) % placedList.length)}>
            ›
          </button>
        </div>
      )}
    </>
  );
}

interface Shared {
  main: Client;
  cell: Cell;
  phone: boolean;
  sizing: boolean;
  addresses: Address[];
  active: Address | null;
  setActive: (a: Address) => void;
  remove: (a: Address) => void;
  grabLeaf: (a: Address, e: PointerEvent) => void;
  elRef: (a: Address) => (el: HTMLElement | null) => void;
  wake: () => void;
}

/** One block: its header and its live pane, or why it can't show it. */
function Leaf({ a, n, s, place, hidden }: { a: Address; n: number; s: Shared; place: Box | undefined; hidden: boolean; index: number }) {
  const at = useMemo(() => parseAddress(a), [a]);
  const [client, setClient] = useState<Client | null>(null);
  const [, bump] = useState(0);
  const [graceOver, setGraceOver] = useState(false);
  const body = useRef<HTMLDivElement>(null);
  const term = useRef<HTMLDivElement>(null);
  const [cells, setCells] = useState<{ cols: number; rows: number } | null>(null);
  const pushed = useRef("");

  // The size to ask for is that of its place, never of the element: in flight
  // that's a card or a tile, and a block on its way home has no place at all.
  // Only a viewer's whole place counts, once a divider or a window stops moving.
  const wantCols = place ? Math.max(2, Math.floor((place.w - 2) / s.cell.width)) : 0;
  const wantRows = place ? Math.max(1, Math.floor((place.h - HEAD_H - 2) / s.cell.height)) : 0;
  useEffect(() => {
    if (!wantCols) return setCells(null);
    const t = setTimeout(() => setCells({ cols: wantCols, rows: wantRows }), SIZE_SETTLE_MS);
    return () => clearTimeout(t);
  }, [wantCols, wantRows]);

  // Its connection, for as long as this block is on the surface.
  useLayoutEffect(() => {
    if (!at) return;
    const token = {};
    const c = viewLinks.take(at.machine, at.pane, token);
    setClient(c);
    const off = c?.subscribe(() => bump((t) => t + 1));
    return () => {
      off?.();
      viewLinks.release(at.machine, at.pane, token);
    };
  }, [at?.machine, at?.pane]);

  const hasState = !!client?.state;
  useEffect(() => {
    if (!hasState) return;
    const t = setTimeout(() => setGraceOver(true), GONE_GRACE_MS);
    return () => clearTimeout(t);
  }, [hasState]);

  const pane = at?.pane ?? -1;
  const info = client?.info(pane);
  const isTerm = !!client?.panes.has(pane);
  const view = client?.panes.get(pane)?.view;
  const host = at ? hostOf(at.machine) : null;
  const note = at
    ? placeholder({
        machine: host ?? at.machine,
        pane,
        known: host !== null && client !== null,
        connected: !!client?.connected,
        hasState,
        hasPane: !!info,
        shared: !!client?.state?.roles,
        graceOver,
      })
    : UNKNOWN_LEAF_NOTE;

  // A terminal keeps its tab's size and is drawn scaled in the leaf; other
  // blocks fill it.
  useLayoutEffect(() => {
    const el = body.current;
    if (!el) return;
    const fit = () => {
      const w = el.clientWidth;
      const h = el.clientHeight;
      if (term.current && view) {
        const tw = view.cols * s.cell.width;
        const th = view.rows * s.cell.height;
        const fitted = Math.min(1, w / (tw || 1), h / (th || 1));
        // On a phone, another screen's wide terminal stays at a size to read,
        // and pans (as a viewer's does in a tab).
        const scale = s.phone ? Math.max(READABLE, Math.min(1, w / (tw || 1))) : fitted;
        term.current.style.width = `${tw}px`;
        term.current.style.height = `${th}px`;
        term.current.style.transform = scale < 1 ? `scale(${scale})` : "";
        // A scaled box keeps its unscaled size in the layout: this is what
        // the leaf's scrollbars and centring should go by.
        term.current.style.marginRight = s.phone ? `${tw * scale - tw}px` : "";
        term.current.style.marginBottom = s.phone ? `${th * scale - th}px` : "";
      }
    };
    fit();
    const ro = new ResizeObserver(fit);
    ro.observe(el);
    return () => ro.disconnect();
  }, [view, view?.cols, view?.rows, s.cell.width, s.cell.height]);

  const tab = client?.tabOfPane(pane);
  const sharers = tab && at ? s.addresses.filter((x) => { const p = parseAddress(x); return p?.machine === at.machine && client?.tabOfPane(p.pane)?.id === tab.id; }).length : 0;
  const viewer = !!client && !!tab && client.role(client.sessionOfTab(tab.id) ?? null) === "viewer";
  const present = !!tab && tab.owner !== null && !!client?.state?.presence?.some((p) => p.client === tab.owner);
  const ownPage = !!tab && !!client && s.main.base === client.base && tab.owner !== null && tab.owner === s.main.clientId;
  const ask = tab && client ? sizeAsk({ owner: tab.owner, me: client.clientId ?? -1, ownPage, present, sharers, viewer }) : "none";
  const zoom = tab && paneIds(tab).length > 1 ? pane : null;

  // Follow the size rule: docs/design/views.md, "Tab-scoped things".
  useEffect(() => {
    if (!s.sizing || !client?.connected || !tab || !cells || !isTerm || ask === "none") return;
    const key = `${client.clientId}:${tab.id}:${cells.cols}x${cells.rows}:${zoom}:${ask}`;
    if (key === pushed.current) return;
    pushed.current = key;
    client.view(tab.id, cells.cols, cells.rows, zoom, ask !== "keep", ask === "ask");
  });

  const elsewhere = s.sizing && isTerm && ask === "none" && !viewer && sharers === 1 && !!tab && tab.owner !== null && tab.owner !== client?.clientId;
  const takeSize = () => {
    if (client && tab && cells) client.view(tab.id, cells.cols, cells.rows, zoom, true, true);
  };

  const fleetish = { info: info ?? {} } as unknown as FleetPane;
  const rgb = info ? KINDS[kindOf(fleetish)] : KINDS.shell;
  const title = !at ? UNKNOWN_LEAF : (client && client.title(pane)) || info?.command || `%${pane}`;
  const { word: type, glyph } = paneGlyph(client as Client, pane, info);
  const status = paneStatus(info);
  const isActive = s.active === a;

  const menu = (e: MouseEvent) => {
    e.preventDefault();
    const items: MenuItem[] = [
      { header: at ? `${title} · ${host ?? at.machine}` : title },
      { label: "Remove from view", run: () => s.remove(a) },
      ...(info && client ? (["separator", { label: "Close pane", danger: true, hint: "Closes it where it lives, not just here", run: () => client.intent({ op: "close_pane", pane }) }] as MenuItem[]) : []),
    ];
    openMenu(e, items);
  };

  return (
    <section
      ref={s.elRef(a)}
      class={`view-leaf${isActive ? " active" : ""}`}
      data-view-leaf={a}
      data-kind-rgb={rgb.join(",")}
      style={{ "--c": rgb.join(" "), display: hidden ? "none" : undefined }}
      aria-label={at ? `${title} on ${host ?? at.machine}` : title}
      onPointerDownCapture={() => s.setActive(a)}
    >
      <header
        class="view-leaf-head"
        tabIndex={0}
        title={`${title}: drag to move, past the edge to send it back`}
        onPointerDown={(e) => {
          if (e.button !== 0 || (e.target as HTMLElement).closest("button") || s.phone) return;
          e.preventDefault();
          const sx = e.clientX;
          const sy = e.clientY;
          const m = (ev: PointerEvent) => {
            if (Math.hypot(ev.clientX - sx, ev.clientY - sy) < 5) return;
            done();
            s.grabLeaf(a, ev);
          };
          const done = () => {
            removeEventListener("pointermove", m, true);
            removeEventListener("pointerup", done, true);
            removeEventListener("pointercancel", done, true);
          };
          addEventListener("pointermove", m, true);
          addEventListener("pointerup", done, true);
          addEventListener("pointercancel", done, true);
        }}
        onKeyDown={(e) => {
          if (e.target === e.currentTarget && (e.key === "Delete" || e.key === "Backspace")) {
            e.preventDefault();
            s.remove(a);
          }
        }}
        onContextMenu={menu}
      >
        {n > 0 && <span class="view-leaf-n">{String(n).padStart(2, "0")}</span>}
        <span class="view-leaf-type" role="img" aria-label={type} title={type}>
          <PaneTypeIcon type={glyph} />
        </span>
        <span class="view-leaf-title">{title}</span>
        {status.word && client?.connected && (
          <span class={`view-leaf-status ${status.cls}`} data-pane-status={status.cls}>
            {status.word}
          </span>
        )}
        {at && <span class="view-leaf-host">{host ?? at.machine}</span>}
        {elsewhere && (
          <button class="view-leaf-size" data-sized-elsewhere title="Another window set this tab's size" disabled={!cells} onClick={takeSize}>
            Use this size
          </button>
        )}
        <button class="view-leaf-button" aria-label="Pane menu" title="Pane menu" onClick={menu}>
          <MoreIcon size={13} />
        </button>
        <button class="view-leaf-button" data-view-remove={a} aria-label={`Remove ${title} from this view`} title="Remove from this view. It keeps running." onClick={() => s.remove(a)}>
          ×
        </button>
      </header>
      <div class={s.phone ? "view-leaf-body panned" : "view-leaf-body"} ref={body} style={{ top: `${HEAD_H}px` }}>
        {note ? (
          <div class="view-gone" data-view-gone={a}>
            <p>{note}</p>
            <button onClick={() => s.remove(a)}>Remove</button>
          </div>
        ) : client && info ? (
          <div class={isTerm ? "view-term" : "view-term fill"} ref={term}>
            <PaneSlot client={client} id={pane} rect={{ x: 0, y: 0, cols: 1, rows: 1 }} cell={s.cell} active={isActive} phone={s.phone} inView={menu} />
          </div>
        ) : (
          <div class="view-gone quiet">
            <p>Connecting to {host ?? at?.machine}…</p>
          </div>
        )}
      </div>
    </section>
  );
}
