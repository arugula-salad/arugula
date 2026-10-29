// arugula.stream's rooms: join by link (/r/<room> or ?room=), a demo token
// from the relay, the Phoenix transport (@arugula/presence), and everyone
// here as live avatars or outlines (@arugula/presence-core). Knock to talk.

import { useEffect, useRef, useState } from "preact/hooks";
import { PresenceClient, phoenixTransport, type Member } from "@arugula/presence";
import { CHARACTERS, characterOf } from "@arugula/presence-core/avatar/characters/index";
import { animate, COLOURS, type Source } from "@arugula/presence-core/avatar/stage";
import { draw } from "@arugula/presence-core/outline/render";
import { palette } from "@arugula/presence-core/outline/classes";
import type { Frame } from "@arugula/presence-core/outline/frame";

export const NAME = "arugula.video.name";

export function kept(key: string): string {
  try {
    return localStorage.getItem(key) ?? "";
  } catch {
    return "";
  }
}

export function keep(key: string, v: string) {
  try {
    localStorage.setItem(key, v);
  } catch {
    // This page still has it.
  }
}

const WORDS = ["quiet", "green", "sunny", "small", "brisk", "calm", "bright", "lucky"];
const THINGS = ["fern", "leaf", "otter", "kite", "pebble", "comet", "maple", "harbor"];
const pick = <T,>(a: T[]) => a[Math.floor(Math.random() * a.length)];
export const newRoom = () => `${pick(WORDS)}-${pick(THINGS)}-${10 + Math.floor(Math.random() * 90)}`;
export const cleanRoom = (r: string) => r.trim().toLowerCase().replace(/[^a-z0-9_.:-]+/g, "-").replace(/^-+|-+$/g, "").slice(0, 64);

function useTick(sub: (fn: () => void) => () => void) {
  const [, set] = useState(0);
  useEffect(() => sub(() => set((n) => n + 1)), []);
}

/** A character idling: the landing's cast, and anyone with no frame yet. */
export function Idle({ character, colour, away = false, class: cls }: { character: number; colour: number; away?: boolean; class?: string }) {
  const ref = useRef<HTMLCanvasElement>(null);
  useEffect(() => animate(ref.current!, { size: "bust", character: characterOf, source: () => ({ rig: null, look: { character, colour }, away }) }), [character, colour, away]);
  return <canvas ref={ref} class={cls} />;
}

export function Join({ room, onName }: { room: string; onName: (n: string) => void }) {
  const [name, setName] = useState(kept(NAME));
  const go = () => name.trim() && onName(name.trim().slice(0, 80));
  return (
    <main class="join">
      <h2>{room}</h2>
      <span class="sub">What should people here call you?</span>
      <input type="text" autoFocus placeholder="your name" value={name} onInput={(e) => setName((e.target as HTMLInputElement).value)} onKeyDown={(e) => e.key === "Enter" && go()} />
      <button class="btn primary" onClick={go} disabled={!name.trim()}>
        Join the room
      </button>
    </main>
  );
}

function OutlineCanvas({ frame, mirror }: { frame: Frame; mirror: boolean }) {
  const ref = useRef<HTMLCanvasElement>(null);
  useEffect(() => {
    const c = ref.current;
    if (c) draw(c, frame, { color: palette(c, !!frame.classes?.some((x) => x)), mirror });
  });
  return <canvas ref={ref} />;
}

function AvatarCanvas({ source, mirror }: { source: () => Source | null; mirror: boolean }) {
  const ref = useRef<HTMLCanvasElement>(null);
  const src = useRef(source);
  src.current = source;
  useEffect(() => animate(ref.current!, { size: "bust", mirror, character: characterOf, source: () => src.current() }), [mirror]);
  return <canvas ref={ref} />;
}

function Tile({ client, m, me }: { client: PresenceClient; m: Member; me: boolean }) {
  const seen = client.of(m.sub);
  const frame = (me ? client.mine?.frame : null) ?? seen?.frame ?? null;
  const look = me ? client.look : (seen?.look ?? client.lookOf(m.sub) ?? m.look);
  const avatar = me ? client.mode === "avatar" : !!look && !frame;
  const live = !!frame || !!seen?.rig || (me && !!client.mine?.rig);
  const knocked = client.answers.find((a) => a.from === m.sub);
  const source = (): Source | null => {
    if (me) return { rig: client.mine?.rig ?? null, look: client.look, away: false };
    const s = client.of(m.sub);
    const l = s?.look ?? client.lookOf(m.sub) ?? m.look;
    return l ? { rig: s?.rig ?? null, look: l, away: m.away } : null;
  };
  return (
    <figure class={live ? "tile live" : "tile"} data-sub={m.sub}>
      <div class="view">
        {frame ? <OutlineCanvas frame={frame} mirror={me} /> : avatar ? <AvatarCanvas source={source} mirror={me} /> : <div class="still">{m.name.slice(0, 1).toUpperCase()}</div>}
      </div>
      <figcaption>
        <span class="who">{me ? `${m.name} (you)` : m.name}</span>
        {live && <span class="tag">live</span>}
        {!live && m.away && <span class="tag" style={{ color: "var(--dim)" }}>away</span>}
        {!me && (
          <button class="btn knock" onClick={() => client.knock(m.sub)} title={`Ask ${m.name} to talk`}>
            {knocked ? (knocked.yes ? "said yes" : "not now") : "Knock"}
          </button>
        )}
      </figcaption>
    </figure>
  );
}

function Controls({ client }: { client: PresenceClient }) {
  const on = client.cameraOn;
  const sent = client.mine?.sent ?? [];
  const last = sent[sent.length - 1];
  const st = client.mine?.stats;
  return (
    <div class="controls">
      <button class={on ? "btn on" : "btn primary"} onClick={() => (on ? client.stopCamera() : void client.startCamera())}>
        {on ? "Turn off my camera" : "Turn on my camera"}
      </button>
      <span class="seg" role="radiogroup" aria-label="What your camera sends">
        {(["avatar", "outline"] as const).map((m) => (
          <button role="radio" aria-checked={client.mode === m} class={client.mode === m ? "btn on" : "btn"} onClick={() => client.setMode(m)}>
            {m === "avatar" ? "Avatar" : "Outline"}
          </button>
        ))}
      </span>
      {client.mode === "avatar" && (
        <>
          <span class="seg" role="radiogroup" aria-label="Your character">
            {CHARACTERS.map((c) => (
              <button role="radio" aria-checked={client.look.character === c.id} class={client.look.character === c.id ? "btn on" : "btn"} onClick={() => client.setLook({ ...client.look, character: c.id })}>
                {c.name}
              </button>
            ))}
          </span>
          <span class="seg" role="radiogroup" aria-label="Your colour">
            {COLOURS.map((v, i) => (
              <button
                role="radio"
                aria-checked={client.look.colour === i}
                aria-label={v.slice(2)}
                class={client.look.colour === i ? "swatch on" : "swatch"}
                style={{ background: `var(${v})` }}
                onClick={() => client.setLook({ ...client.look, colour: i })}
              />
            ))}
          </span>
        </>
      )}
      {on && client.mode === "avatar" && last && (
        <span class="stats">
          rig {last.bytes} B · {client.kbps().toFixed(1)} kbps · tracking {client.mine?.trackMs.toFixed(0)} ms
        </span>
      )}
      {on && client.mode === "outline" && st && (
        <span class="stats">
          {st.lines} lines · {(st.bytes / 1024).toFixed(1)} KB · {client.kbps().toFixed(1)} kbps
        </span>
      )}
      {on && !last && !client.mine?.error && <span class="stats">starting the camera…</span>}
      {client.mine?.error && <span class="error">{client.mine.error}</span>}
    </div>
  );
}

export function Room({ room, name }: { room: string; name: string }) {
  const [state] = useState(() => {
    let sub = "";
    const token = async () => {
      const r = await fetch("/api/demo/token", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ room, name }) });
      if (!r.ok) throw new Error(`the relay said ${r.status}`);
      const t = (await r.json()) as { token: string; sub: string };
      sub = t.sub;
      return t.token;
    };
    const url = `${location.protocol === "https:" ? "wss" : "ws"}://${location.host}/socket`;
    const client = new PresenceClient({ transport: phoenixTransport({ url, token, room }), storage: "arugula.video." });
    try {
      if (!localStorage.getItem("arugula.video.mode")) client.setMode("avatar");
    } catch {
      // Avatar by default where nothing's kept.
    }
    return { client, me: () => sub };
  });
  const { client } = state;
  useTick((fn) => client.subscribe(fn));
  useEffect(() => {
    const stop = client.watch();
    const away = () => client.setAway(document.hidden);
    addEventListener("visibilitychange", away);
    return () => {
      stop();
      removeEventListener("visibilitychange", away);
    };
  }, []);
  const [copied, setCopied] = useState(false);
  const me = state.me();
  const mine: Member = client.members.find((m) => m.sub === me) ?? { sub: me || "me", name, look: client.look, mode: "none", away: false, watching: true };
  const others = client.members.filter((m) => m.sub !== me);
  // Names of everyone seen here, so a knock from someone who's since left
  // still says who.
  const names = useRef(new Map<string, string>()).current;
  for (const m of client.members) names.set(m.sub, m.name);
  const knock = client.knocks[client.knocks.length - 1];
  const share = () => {
    void navigator.clipboard?.writeText(location.href).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    });
  };
  return (
    <main class="room">
      <div class="bar">
        <span class="name">
          <a href="/">arugula.stream</a>
          <small>{room}</small>
        </span>
        <span class="stats">{client.live === "on" ? `${client.members.length} here` : client.live === "refused" ? "the relay refused" : "connecting…"}</span>
        <button class="btn" onClick={share}>
          {copied ? "Link copied" : "Copy the room link"}
        </button>
      </div>
      <Controls client={client} />
      <div class="grid">
        <Tile client={client} m={mine} me />
        {others.map((m) => (
          <Tile key={m.sub} client={client} m={m} me={false} />
        ))}
      </div>
      {!others.length && <p class="empty">Nobody else here yet. Send someone the room link.</p>}
      {knock && (
        <div class="toast" role="alert">
          <span>
            <b>{names.get(knock.from) ?? "Someone"}</b> wants to talk.
          </span>
          <div class="row">
            <button class="btn primary" onClick={() => client.answer(knock.from, true)}>
              Sure
            </button>
            <button class="btn" onClick={() => client.answer(knock.from, false)}>
              Not now
            </button>
          </div>
          <span class="stats">Calls come next: for now, say hi in your room.</span>
        </div>
      )}
    </main>
  );
}

