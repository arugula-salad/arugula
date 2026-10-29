// Who's around, live (People): a team's room on arugula.stream, drawn by
// its own elements (<arugula-room>, <arugula-people>, <arugula-camera>):
// a tile per member in the room, their avatar or outline while their camera
// is on, waves and calls. The daemon keeps nothing but which team it is
// (crates/daemon/src/people_block.rs, config `{team}`); the room is the
// relay's, joined with a token control signs for the person looking
// (`POST /api/chat/presence/token`, members only).
//
// The elements load from the relay's own site the first time a People
// block is shown (control's CSP lets its scripts in only when control has
// an arugula.stream app), and the room is joined only while the block is
// shown: a People block in a tab nobody has open is in no room.

import { render } from "preact";
import { useEffect, useRef, useState } from "preact/hooks";
import type { Client } from "../client";
import type { PaneId } from "../../shared/proto";
import { platformFetch, refusalOf } from "../../shared/maintenance";
import { askText } from "../../shared/ui/menu";
import { useSubscribe } from "../../shared/ui/hooks";
import { getControlSession } from "../ui/people";
import { registerBlock, type BlockView } from "./view";
import "./people.css";

const TOKEN_URL = "/api/chat/presence/token";

/** The block's state: its layout entry, which the daemon says whole. */
export interface PeopleState {
  team: string;
}

/** The relay's https origin, from its socket's URL. */
function httpOrigin(relay: string): string {
  return new URL(relay.replace(/^ws/, "http")).origin;
}

/** arugula.stream's elements, loaded once from the relay's site. */
let loaded: Promise<unknown> | null = null;
function loadElements(relay: string) {
  loaded ??= import(/* @vite-ignore */ `${httpOrigin(relay)}/sdk/v0/presence.js`).catch((e) => {
    loaded = null;
    throw e;
  });
  return loaded;
}

type Joined = { relay: string } | { error: string };

/** Whether control will sign this person into `room`, and the relay it's on. */
async function ask(room: string): Promise<Joined> {
  const r = await platformFetch(TOKEN_URL, {
    method: "POST",
    credentials: "same-origin",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ room }),
  });
  const body = (await r.json().catch(() => ({}))) as { relay?: string; error?: string; reason?: string };
  if (!r.ok || !body.relay) return { error: refusalOf(r.status, body, `Control said ${r.status}`) };
  return { relay: body.relay };
}

function Room({ team, relay }: { team: string; relay: string }) {
  const ref = useRef<HTMLDivElement>(null);
  const [failed, setFailed] = useState<string | null>(null);
  useEffect(() => {
    const host = ref.current;
    if (!host) return;
    let gone = false;
    loadElements(relay)
      .then(() => {
        if (gone) return;
        // Made by hand: the room joins when it's connected, and leaves when
        // it's taken out.
        const room = document.createElement("arugula-room");
        room.setAttribute("room", `team:${team}`);
        room.setAttribute("token-url", TOKEN_URL);
        room.setAttribute("relay", relay);
        room.append(document.createElement("arugula-camera"), document.createElement("arugula-people"));
        host.replaceChildren(room);
      })
      .catch(() => !gone && setFailed(`arugula.stream's elements didn't load from ${httpOrigin(relay)}`));
    return () => {
      gone = true;
      host.replaceChildren();
    };
  }, [team, relay]);
  return failed ? <p class="people-none dim">{failed}</p> : <div ref={ref} class="people-room" />;
}

function PeopleBlock({ s, shown }: { s: PeopleState | null; shown: boolean }) {
  const session = getControlSession();
  useSubscribe((fn) => session?.subscribe(fn) ?? (() => {}));
  const ready = session?.phase === "ready";
  const [joined, setJoined] = useState<Joined | null>(null);
  const team = s?.team;
  useEffect(() => {
    if (!team || !ready) return;
    let gone = false;
    ask(`team:${team}`)
      .then((j) => !gone && setJoined(j))
      .catch(() => !gone && setJoined({ error: "Control didn't answer" }));
    return () => {
      gone = true;
    };
  }, [team, ready]);
  const name = session?.teams.find((t) => t.team === team)?.roster.name;
  let body;
  if (!s) body = <p class="people-none dim">…</p>;
  else if (!ready)
    body = (
      <p class="people-none dim" data-people-needs-control>
        Who's around is your team's, on Arugula control, and you join as yourself. Open this machine through control's page, signed in, to see them here.
      </p>
    );
  else if (!joined) body = <p class="people-none dim">…</p>;
  else if ("error" in joined) body = <p class="people-none dim" data-people-refused>{joined.error}</p>;
  else if (shown) body = <Room team={s.team} relay={joined.relay} />;
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
