// Who's around, as live camera outlines (S43): a tile per person you share
// a team with, their outline while their camera is on, otherwise their
// picture and presence dot. The daemon keeps nothing but that the block is
// there (crates/daemon/src/people_block.rs, config `{}`); the frames are
// control's to relay (../../shared/outline/store), read as the person
// looking, and the stream runs only while the block is shown.
//
// Your own camera is off until you turn it on here, and it's only lines
// that leave the page.

import { render } from "preact";
import { useEffect, useRef } from "preact/hooks";
import type { Client } from "../client";
import type { PaneId } from "../../shared/proto";
import { MsgAvatar } from "../../shared/chat/avatar";
import { presence } from "../../shared/chat/presence";
import { outlines } from "../../shared/outline/store";
import { draw } from "../../shared/outline/render";
import type { Frame } from "../../shared/outline/frame";
import { GROUPS, groupOf } from "../../shared/outline/classes";
import { useSubscribe } from "../../shared/ui/hooks";
import { getControlSession } from "../ui/people";
import { registerBlock, type BlockView } from "./view";
import "../../shared/outline/people.css";

const RATES = [2, 4, 8, 15];
const ORDER = { active: 0, away: 1, offline: 2 } as const;

interface Person {
  account: string;
  name: string;
}

/** Everyone on the reader's teams, once each, the reader first. */
function everyone(): Person[] {
  const s = getControlSession();
  if (!s) return [];
  const out = new Map<string, Person>();
  if (s.account) out.set(s.account, { account: s.account, name: "You" });
  for (const t of s.teams) {
    if (t.role === null) continue;
    for (const m of t.roster.members) if (!out.has(m.account)) out.set(m.account, { account: m.account, name: t.names?.[m.account] ?? m.name });
  }
  return [...out.values()];
}

/** A frame's colours from the theme: each group's, and for the rest the
 * accent (a frame nobody labelled) or dim (labelled, but not one we
 * colour). */
function palette(el: Element, labelled: boolean): (cls: number) => string {
  const css = getComputedStyle(el);
  const of = (v: string) => css.getPropertyValue(v).trim() || css.color;
  const groups = GROUPS.map((g) => of(g.css));
  const rest = labelled ? of("--dim") : of("--accent");
  return (cls) => {
    const g = groupOf(cls);
    return g < 0 ? rest : groups[g];
  };
}

function OutlineCanvas({ frame, mirror }: { frame: Frame; mirror?: boolean }) {
  const ref = useRef<HTMLCanvasElement>(null);
  useEffect(() => {
    const c = ref.current;
    if (!c) return;
    draw(c, frame, { color: palette(c, !!frame.classes?.some((x) => x)), mirror });
  });
  return <canvas ref={ref} class="people-canvas" aria-hidden="true" />;
}

function Tile({ p, me }: { p: Person; me: boolean }) {
  const mine = me ? outlines.mine : null;
  const seen = outlines.of(p.account);
  const frame = mine?.frame ?? seen?.frame ?? null;
  const state = presence.of(p.account) ?? (me ? "active" : "offline");
  const live = !!frame;
  return (
    <figure class={`people-tile people-${state}${live ? " people-live" : ""}`} data-people-tile={p.account} data-live={live || undefined}>
      <div class="people-view">
        {frame ? (
          <OutlineCanvas frame={frame} mirror={me} />
        ) : (
          <div class="people-still">
            <MsgAvatar who={p.account} name={p.name} account={p.account} dot />
          </div>
        )}
      </div>
      <figcaption>
        <span class={`presence-dot presence-${state}`} aria-hidden="true" />
        <span class="people-name">{p.name}</span>
        {live && <span class="people-on">live</span>}
        {!me && seen && <span class="people-bytes dim">{(seen.bytes / 1024).toFixed(1)} KB</span>}
      </figcaption>
    </figure>
  );
}

function Controls() {
  const on = outlines.cameraOn;
  const st = outlines.mine?.stats;
  const err = outlines.mine?.error;
  return (
    <div class="people-controls">
      <button
        type="button"
        class={on ? "btn people-cam on" : "btn primary people-cam"}
        data-people-camera={on ? "on" : "off"}
        onClick={() => (on ? outlines.stopCamera() : void outlines.startCamera())}
      >
        {on ? "Turn off my outline" : "Turn on my outline"}
      </button>
      <label class="people-rate">
        <span class="dim">fps</span>
        <select value={String(outlines.fps)} onChange={(e) => outlines.setRate(Number((e.target as HTMLSelectElement).value))}>
          {RATES.map((r) => (
            <option value={String(r)}>{r}</option>
          ))}
        </select>
      </label>
      <label class="people-label" title="Colour each line by what it traces: you, your chair, your screen (MediaPipe, in this page)">
        <input type="checkbox" checked={outlines.label} onChange={(e) => outlines.setLabel((e.target as HTMLInputElement).checked)} />
        <span>colour by what it is</span>
        {on && outlines.labelState === "loading" && <span class="dim">(loading…)</span>}
      </label>
      <label class="people-detail">
        <span class="dim">detail</span>
        <input
          type="range"
          min="0"
          max="1"
          step="0.05"
          value={outlines.detail}
          onInput={(e) => outlines.setDetail(Number((e.target as HTMLInputElement).value))}
        />
      </label>
      {st && (
        <span class="people-stats dim" data-people-stats>
          {st.lines} lines · {st.points} pts · {(st.raw / 1024).toFixed(1)} KB raw
          {st.sent ? ` → ${(st.bytes / 1024).toFixed(1)} KB` : " · still, held"} · {outlines.kbps().toFixed(1)} kbps · {st.ms.toFixed(0)} ms
          {st.labelMs !== null && ` + ${st.labelMs.toFixed(0)} ms labels`}
        </span>
      )}
      {err && <span class="people-error">{err}</span>}
      {outlines.labelError && <span class="people-error">{outlines.labelError}</span>}
      <span class="people-legend" aria-label="What the colours mean">
        {GROUPS.map((g) => (
          <span class="people-key">
            <i style={{ background: `var(${g.css})` }} />
            {g.name}
          </span>
        ))}
      </span>
    </div>
  );
}

function PeopleBlock({ shown }: { shown: boolean }) {
  const session = getControlSession();
  useSubscribe((fn) => session?.subscribe(fn) ?? (() => {}));
  useSubscribe((fn) => outlines.subscribe(fn));
  useSubscribe((fn) => presence.subscribe(fn));
  const ready = session?.phase === "ready";
  useEffect(() => {
    if (!ready || !shown) return;
    // Presence runs with chat; a People block wants it either way.
    if (!presence.on) presence.start();
    return outlines.watch();
  }, [ready, shown]);
  if (!ready) {
    return (
      <p class="people-none dim" data-people-needs-control>
        Who's around is your teams', on Arugula control. Open this machine through control's page, signed in, to see them here.
      </p>
    );
  }
  const me = session.account;
  const people = everyone().sort((a, b) => {
    if (a.account === me) return -1;
    if (b.account === me) return 1;
    const la = outlines.of(a.account) ? 0 : 1, lb = outlines.of(b.account) ? 0 : 1;
    if (la !== lb) return la - lb;
    const sa = ORDER[presence.of(a.account) ?? "offline"], sb = ORDER[presence.of(b.account) ?? "offline"];
    return sa - sb || a.name.localeCompare(b.name);
  });
  return (
    <div class="people-block" data-people-live={outlines.live}>
      <Controls />
      {outlines.live === "refused" && <p class="people-error">Control refused the outlines stream: chat has to be on for one of your teams.</p>}
      <div class="people-grid">
        {people.map((p) => (
          <Tile key={p.account} p={p} me={p.account === me} />
        ))}
      </div>
    </div>
  );
}

/** "People": a block of who's around, beside `from`. */
export async function openPeople(client: Client, o: { from: PaneId }) {
  await client.openBlock({ type: "people", config: {}, local: true, split: o.from, from_pane: o.from }, "couldn't open People");
}

registerBlock("people", (_client: Client, _id: PaneId): BlockView => {
  const host = document.createElement("div");
  host.className = "block block-people";
  let visible = true;
  const paint = () => render(<PeopleBlock shown={visible} />, host);
  paint();
  return {
    host,
    setVisible(v) {
      host.style.display = v ? "" : "none";
      if (v !== visible) {
        visible = v;
        paint();
      }
    },
    update() {
      paint();
    },
    title: () => "People",
    text: () => host.textContent ?? "",
    focus: () => host.querySelector<HTMLElement>("button")?.focus(),
    dispose() {
      render(null, host);
      host.remove();
    },
  };
});
