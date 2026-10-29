// Drawing a room's people into an element (connect()'s mount and avatar,
// and the elements): a tile each, their outline while their camera sends
// one, their character (live, or idling in their look) otherwise, their
// initial when they have neither. Plain DOM and canvases, no framework.

import type { Member } from "@arugula/presence";
import { characterOf } from "@arugula/presence-core/avatar/characters/index";
import { animate, type Source } from "@arugula/presence-core/avatar/stage";
import { waving, type Rig } from "@arugula/presence-core/avatar/rig";
import { draw } from "@arugula/presence-core/outline/render";
import { palette } from "@arugula/presence-core/outline/classes";
import type { Frame } from "@arugula/presence-core/outline/frame";
import type { Room } from "./room.ts";

export interface PeopleOptions {
  /** A bust each (default) or a compact strip of heads. */
  size?: "bust" | "head";
  /** You first (default), or not shown. */
  self?: "first" | "hide";
}

export interface AvatarOptions {
  size?: "head" | "bust";
}

type Kind = "outline" | "avatar" | "initial";

/** What one person's view draws from now. */
function what(room: Room, m: Member): { kind: Kind; frame: Frame | null; source: () => Source | null } {
  const c = room.client;
  const me = m.sub === room.me;
  const seen = c.of(m.sub);
  const frame = (me ? c.mine?.frame : null) ?? seen?.frame ?? null;
  // A fresh wave (theirs at you, yours at someone) waves on their avatar.
  const hand = (rig: Rig | null) => (c.waving(m.sub, me) ? waving(rig, performance.now() / 1000) : rig);
  const source = (): Source | null => {
    if (me) return { rig: hand(c.mine?.rig ?? null), look: c.look, away: false };
    const s = c.of(m.sub);
    const now = c.members.find((x) => x.sub === m.sub) ?? m;
    const look = s?.look ?? c.lookOf(m.sub) ?? now.look;
    return look ? { rig: hand(s?.rig ?? null), look, away: now.away } : null;
  };
  const kind: Kind = frame ? "outline" : (me ? c.mode === "avatar" : !!source()) ? "avatar" : "initial";
  return { kind, frame, source };
}

/** One person's canvas (or initial) inside `view`, redrawn as they change. */
class Face {
  private kind: Kind | null = null;
  private stopAnim: (() => void) | null = null;
  private canvas: HTMLCanvasElement | null = null;
  private frame: Frame | null = null;
  private view: HTMLElement;
  private size: "bust" | "head";
  constructor(view: HTMLElement, size: "bust" | "head") {
    this.view = view;
    this.size = size;
  }

  update(room: Room, m: Member) {
    const w = what(room, m);
    const me = m.sub === room.me;
    if (w.kind !== this.kind) {
      this.clear();
      this.kind = w.kind;
      if (w.kind === "initial") {
        const i = document.createElement("span");
        i.className = "initial";
        i.textContent = (m.name || "?").slice(0, 1).toUpperCase();
        this.view.append(i);
      } else {
        this.canvas = document.createElement("canvas");
        this.view.append(this.canvas);
        if (w.kind === "avatar") this.stopAnim = animate(this.canvas, { size: this.size, mirror: me, character: characterOf, source: w.source });
      }
    }
    if (w.kind === "outline" && this.canvas && w.frame && w.frame !== this.frame) {
      this.frame = w.frame;
      const c = this.canvas;
      draw(c, w.frame, { color: palette(c, !!w.frame.classes?.some((x) => x)), mirror: me, fit: this.size === "head" ? "cover" : "contain" });
    }
  }

  clear() {
    this.stopAnim?.();
    this.stopAnim = null;
    this.canvas = null;
    this.frame = null;
    this.view.replaceChildren();
  }
}

interface Tile {
  fig: HTMLElement;
  face: Face;
  name: HTMLElement;
  tag: HTMLElement;
}

export function mountPeople(room: Room, el: Element | ShadowRoot, o: PeopleOptions = {}): () => void {
  const size = o.size ?? "bust";
  const grid = document.createElement("div");
  grid.className = `arugula-people ${size}`;
  grid.setAttribute("part", "grid");
  el.append(grid);
  const tiles = new Map<string, Tile>();
  const render = () => {
    const c = room.client;
    let people = c.members;
    if (room.me && !people.some((m) => m.sub === room.me) && c.live === "on") {
      people = [{ sub: room.me, name: "You", look: c.look, mode: "none", away: false, watching: true }, ...people];
    }
    people = [...people].sort((a, b) => (a.sub === room.me ? -1 : b.sub === room.me ? 1 : 0));
    if (o.self === "hide") people = people.filter((m) => m.sub !== room.me);
    const keep = new Set(people.map((m) => m.sub));
    for (const [sub, t] of tiles) {
      if (!keep.has(sub)) {
        t.face.clear();
        t.fig.remove();
        tiles.delete(sub);
      }
    }
    people.forEach((m, i) => {
      let t = tiles.get(m.sub);
      if (!t) {
        const fig = document.createElement("figure");
        fig.className = "tile";
        fig.setAttribute("part", "tile");
        fig.dataset.sub = m.sub;
        const view = document.createElement("div");
        view.className = "view";
        const cap = document.createElement("figcaption");
        const name = document.createElement("span");
        name.className = "name";
        const tag = document.createElement("span");
        tag.className = "tag";
        cap.append(name, tag);
        fig.append(view, cap);
        t = { fig, face: new Face(view, size), name, tag };
        tiles.set(m.sub, t);
      }
      if (grid.children[i] !== t.fig) grid.insertBefore(t.fig, grid.children[i] ?? null);
      const me = m.sub === room.me;
      t.name.textContent = me ? `${m.name} (you)` : m.name;
      const seen = c.of(m.sub);
      const live = !!seen || (me && !!(c.mine?.rig || c.mine?.frame));
      t.fig.classList.toggle("live", live);
      t.fig.classList.toggle("away", m.away && !live);
      t.tag.textContent = live ? "live" : m.away ? "away" : "";
      t.face.update(room, m);
    });
  };
  const unsub = room.client.subscribe(render);
  render();
  return () => {
    unsub();
    for (const t of tiles.values()) t.face.clear();
    grid.remove();
  };
}

export function mountAvatar(room: Room, el: Element | ShadowRoot, sub: string, o: AvatarOptions = {}): () => void {
  const view = document.createElement("div");
  view.className = `arugula-avatar ${o.size ?? "head"}`;
  view.setAttribute("part", "avatar");
  el.append(view);
  const face = new Face(view, o.size === "bust" ? "bust" : "head");
  const render = () => {
    const m = room.client.members.find((x) => x.sub === sub) ?? { sub, name: sub, look: null, mode: "none" as const, away: false, watching: false };
    face.update(room, m);
  };
  const unsub = room.client.subscribe(render);
  render();
  return () => {
    unsub();
    face.clear();
    view.remove();
  };
}
