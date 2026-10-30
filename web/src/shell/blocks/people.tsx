// Who's around, live (People): a team's room on arugula.stream, drawn by
// its own elements (<arugula-room>, <arugula-people>, <arugula-camera>):
// a tile per member in the room, their avatar or outline while their camera
// is on, waves, the room's voice channel (<arugula-voice>: join muted,
// unmute, video) and a wave's private call (<arugula-call>), where real
// video stands in for the avatar on the tile. In a call or voice there's
// Grid or Speaker view (<arugula-spotlight>: whoever's talking big, the
// rest in a strip). The daemon keeps nothing but which team it is
// (crates/daemon/src/people_block.rs, config `{team}`); the room is the
// relay's, joined with a token control signs for the person looking
// (`POST /api/chat/presence/token`, members only).
//
// The room is the page's live presence connection's (shared/live, #346):
// the block holds its team's room open while it's shown, whatever the
// live presence switch says, and the elements draw from that room
// (`el.room`), so live avatars elsewhere on the page and this block are
// one member in it. A People block in a tab nobody has open holds
// nothing. A room that's connecting, refused or out of reach says so, with
// Try again, rather than leaving the block blank.

import { render } from "preact";
import { useEffect, useRef, useState } from "preact/hooks";
import type { Client } from "../client";
import type { PaneId } from "../../shared/proto";
import { askText } from "../../shared/ui/menu";
import { useSubscribe } from "../../shared/ui/hooks";
import { getControlSession } from "../ui/people";
import { live } from "../../shared/live/live";
import type { RoomState, SdkRoom } from "../../shared/live/store";
import { registerBlock, type BlockView } from "./view";
import "./people.css";

/** The block's state: its layout entry, which the daemon says whole. */
export interface PeopleState {
  team: string;
}

/** Grid or Speaker view, while you're in a call or the room's voice. */
type Layout = "grid" | "speaker";
const LAYOUT_KEY = "arugula.people.layout";

function keptLayout(): Layout {
  try {
    return localStorage.getItem(LAYOUT_KEY) === "speaker" ? "speaker" : "grid";
  } catch {
    return "grid";
  }
}

function keepLayout(l: Layout) {
  try {
    localStorage.setItem(LAYOUT_KEY, l);
  } catch {
    // A private window: it's this block's for now.
  }
}

function LayoutToggle({ layout, onLayout }: { layout: Layout; onLayout: (l: Layout) => void }) {
  return (
    <div class="people-layout">
      <span class="people-seg" role="radiogroup" aria-label="Layout">
        {(["grid", "speaker"] as const).map((l) => (
          <button type="button" role="radio" aria-checked={layout === l} data-people-layout={l} onClick={() => onLayout(l)}>
            {l === "grid" ? "Grid" : "Speaker"}
          </button>
        ))}
      </span>
      {layout === "speaker" && <span class="dim">Click someone to pin them big; again to follow whoever's talking.</span>}
    </div>
  );
}

function Room({ sdkRoom }: { sdkRoom: SdkRoom }) {
  const ref = useRef<HTMLDivElement>(null);
  const [room, setRoom] = useState<HTMLElement | null>(null);
  const [talking, setTalking] = useState(false);
  const [layout, setLayout] = useState<Layout>(keptLayout);
  useEffect(() => {
    const host = ref.current;
    if (!host) return;
    let inCall = false;
    let inVoice = false;
    const heard = (e: Event) => {
      const d = (e as CustomEvent<{ call?: unknown; joined?: unknown }>).detail ?? {};
      if (e.type === "arugula:call") inCall = !!d.call;
      else inVoice = !!d.joined;
      setTalking(inCall || inVoice);
    };
    // Made by hand, on the page's connected room (`room`, an SDK property):
    // taking it out doesn't leave the room; the live presence store does,
    // once nothing holds it.
    const r = document.createElement("arugula-room") as HTMLElement & { room: SdkRoom | null };
    r.room = sdkRoom;
    const bar = document.createElement("div");
    bar.className = "people-layout-slot";
    const el = (name: string) => document.createElement(name);
    r.append(el("arugula-camera"), el("arugula-voice"), el("arugula-call"), bar, el("arugula-people"));
    r.addEventListener("arugula:call", heard);
    r.addEventListener("arugula:voice", heard);
    host.replaceChildren(r);
    setRoom(r);
    return () => {
      render(null, bar);
      host.replaceChildren();
      setRoom(null);
      setTalking(false);
    };
  }, [sdkRoom]);
  // Speaker view: whoever's talking big (<arugula-spotlight>, there only
  // while it's shown, so nobody sends 720p for a hidden tile) and the rest
  // of the room as heads under it. Grid: a tile each.
  const speaker = talking && layout === "speaker";
  useEffect(() => {
    if (!room) return;
    const bar = room.querySelector<HTMLElement>(".people-layout-slot");
    const choose = (l: Layout) => {
      keepLayout(l);
      setLayout(l);
    };
    if (bar) render(talking ? <LayoutToggle layout={layout} onLayout={choose} /> : null, bar);
    const spot = room.querySelector("arugula-spotlight");
    if (speaker && !spot) {
      // <arugula-people> can't leave out who's in voice, so the room under
      // the strip says it's everyone.
      const rest = document.createElement("p");
      rest.className = "people-rest dim";
      rest.textContent = "Everyone in the room";
      bar?.after(document.createElement("arugula-spotlight"), rest);
    }
    if (!speaker) {
      spot?.remove();
      room.querySelector(".people-rest")?.remove();
    }
    // <arugula-people> reads its size when it's put in: a new one for the other.
    const people = room.querySelector("arugula-people");
    if (people && (people.getAttribute("size") === "head") !== speaker) {
      const next = document.createElement("arugula-people");
      if (speaker) next.setAttribute("size", "head");
      people.replaceWith(next);
    }
  }, [room, talking, layout, speaker]);
  return <div ref={ref} class="people-room" />;
}

/** What the block says of its room while it isn't drawing it. */
function RoomNote({ team, state, why }: { team: string; state: RoomState; why: string }) {
  if (state === "connecting") return <p class="people-none dim" data-people-connecting>Joining your team's room…</p>;
  const said = state === "refused" ? why || "arugula.stream refused this room" : "Couldn't reach arugula.stream";
  return (
    <p class="people-none dim" data-people-refused={state}>
      {said}{" "}
      <button type="button" class="link" data-people-retry onClick={() => live.retry(team)}>
        Try again
      </button>
    </p>
  );
}

function PeopleBlock({ s, shown }: { s: PeopleState | null; shown: boolean }) {
  const session = getControlSession();
  useSubscribe((fn) => session?.subscribe(fn) ?? (() => {}));
  useSubscribe((fn) => live.subscribe(fn));
  const ready = session?.phase === "ready";
  const team = s?.team;
  // The block holds its team's room open while it's shown, whatever the
  // live presence switch says (#346).
  useEffect(() => {
    if (!team || !ready || !shown) return;
    return live.hold(team);
  }, [team, ready, shown]);
  const name = session?.teams.find((t) => t.team === team)?.roster.name;
  const at = team ? live.room(team) : null;
  let body;
  if (!s) body = <p class="people-none dim">…</p>;
  else if (!ready)
    body = (
      <p class="people-none dim" data-people-needs-control>
        Who's around is your team's, on Arugula control, and you join as yourself. Open this machine through control's page, signed in, to see them here.
      </p>
    );
  else if (!shown) body = null;
  else if (!at) body = <p class="people-none dim">…</p>;
  // A room that drops keeps drawing (the elements say they're reconnecting).
  else if (at.room) body = <Room sdkRoom={at.room} />;
  else body = <RoomNote team={s.team} state={at.state} why={at.why} />;
  return (
    <div class="people-block" data-people-block={s?.team}>
      {name && <header class="people-head dim">{name}</header>}
      {body}
    </div>
  );
}

/** "People": who's around in one of the reader's teams, beside `from`. */
export async function openPeople(client: Client, o: { from: PaneId }) {
  const session = getControlSession();
  if (session?.phase !== "ready") {
    return client.toast("Who's around is read as you, through Arugula control: open this machine through control's page, signed in");
  }
  const teams = session.teams.filter((t) => t.role !== null).map((t) => ({ value: t.team, label: t.roster.name }));
  if (!teams.length) return client.toast("You're in no team yet: People shows who's around in one");
  let team = teams[0].value;
  if (teams.length > 1) {
    const v = (await askText("People of which team", "", "team", Promise.resolve(teams)))?.trim();
    if (!v) return;
    const hit = teams.find((t) => t.value === v || t.label.toLowerCase() === v.toLowerCase());
    if (!hit) return client.toast(`You're in no team called ${v}`);
    team = hit.value;
  }
  await client.openBlock({ type: "people", config: { team }, local: true, split: o.from, from_pane: o.from }, "couldn't open People");
}

registerBlock("people", (_client: Client, _id: PaneId): BlockView => {
  const host = document.createElement("div");
  host.className = "block block-people";
  let state: PeopleState | null = null;
  let visible = true;
  const draw = () => render(<PeopleBlock s={state} shown={visible} />, host);
  draw();
  return {
    host,
    setVisible(v) {
      host.style.display = v ? "" : "none";
      if (v !== visible) {
        visible = v;
        draw();
      }
    },
    update(s) {
      state = s as PeopleState;
      draw();
    },
    title: () => "People",
    text: () => host.textContent ?? "",
    focus: () => host.querySelector<HTMLElement>("button, input")?.focus(),
    dispose() {
      render(null, host);
      host.remove();
    },
  };
});
