// meet's front door: come in as a guest (a name, kept in this browser) or
// sign in (an emailed link, or Google), start a room, and, signed in, the
// rooms you made and the ones you've been in lately. Guests can start
// rooms too; they just don't get a list.

import { useEffect, useRef, useState } from "preact/hooks";
import { CHARACTERS, characterOf } from "@arugula/presence-core/avatar/characters/index";
import { animate, COLOURS } from "@arugula/presence-core/avatar/stage";
import * as api from "./api";
import { NAME, accountName, go, keep, kept, path, type RoomInfo, type Session } from "./api";

/** A character idling: the home page's cast. */
export function Idle({ character, colour, class: cls }: { character: number; colour: number; class?: string }) {
  const ref = useRef<HTMLCanvasElement>(null);
  useEffect(() => animate(ref.current!, { size: "bust", character: characterOf, source: () => ({ rig: null, look: { character, colour }, away: false }) }), [character, colour]);
  return <canvas ref={ref} class={cls} />;
}

/** meet's name, linking home. */
export function Mark() {
  return (
    <a class="mark" href={path("/")} onClick={(e) => (e.preventDefault(), go(path("/")))}>
      meet<span>.</span>
      <small>arugula.stream</small>
    </a>
  );
}

/** Sign in by an emailed link (back to `returnTo`), or Google where it's set up. */
export function SignIn({ me, returnTo }: { me: Session; returnTo: string }) {
  const [email, setEmail] = useState("");
  const [sent, setSent] = useState("");
  const [err, setErr] = useState("");
  const [busy, setBusy] = useState(false);
  const send = async () => {
    const e = email.trim();
    if (!e || busy) return;
    setBusy(true);
    try {
      await api.emailLink(e, returnTo);
      setSent(e);
      setErr("");
    } catch (x) {
      setErr((x as Error).message);
    } finally {
      setBusy(false);
    }
  };
  if (!me.sign_in.email && !me.sign_in.google) return <p class="sub">Signing in isn't set up here yet. Come in as a guest.</p>;
  if (sent)
    return (
      <p class="sent" role="status">
        Check your email: a sign-in link is on its way to <b>{sent}</b>. It works once, for 15 minutes.
      </p>
    );
  return (
    <div class="signin">
      {me.sign_in.email && (
        <form
          class="row"
          onSubmit={(e) => {
            e.preventDefault();
            void send();
          }}
        >
          <input type="email" placeholder="you@example.com" aria-label="Your email" autocomplete="email" value={email} onInput={(e) => setEmail((e.target as HTMLInputElement).value)} />
          <button class="btn" type="submit" disabled={!email.trim() || busy}>
            Email me a link
          </button>
        </form>
      )}
      {me.sign_in.google && (
        <a class="btn google" href={api.googleUrl(returnTo)}>
          Sign in with Google
        </a>
      )}
      {err && <span class="error">{err}</span>}
    </div>
  );
}

function RoomList({ title, rooms, kind, empty }: { title: string; rooms: RoomInfo[]; kind: string; empty: string }) {
  return (
    <section class="rooms">
      <h2>{title}</h2>
      {!rooms.length && <p class="empty">{empty}</p>}
      <ul>
        {rooms.map((r) => (
          <li key={r.slug}>
            <a class="card room-card" style={{ "--c": `var(${kind})` }} href={path(`/r/${r.slug}`)} onClick={(e) => (e.preventDefault(), go(path(`/r/${r.slug}`)))}>
              <b>{r.name}</b>
              <span class="stats">
                {r.slug} · {when(r.joined_at ?? r.created_at)}
              </span>
            </a>
          </li>
        ))}
      </ul>
    </section>
  );
}

function when(iso: string): string {
  const d = new Date(iso);
  const days = Math.floor((Date.now() - d.getTime()) / 86_400_000);
  if (days < 1) return d.toLocaleTimeString([], { hour: "numeric", minute: "2-digit" });
  if (days < 7) return d.toLocaleDateString([], { weekday: "short" });
  return d.toLocaleDateString([], { month: "short", day: "numeric" });
}

/** A new room's name and its button: makes it and goes in. */
function StartRoom({ who }: { who: string }) {
  const [name, setName] = useState("");
  const [err, setErr] = useState("");
  const [busy, setBusy] = useState(false);
  const start = async () => {
    if (busy) return;
    setBusy(true);
    try {
      const room = await api.createRoom(name.trim() || `${who}'s room`);
      go(path(`/r/${room.slug}`));
    } catch (e) {
      setErr((e as Error).message);
      setBusy(false);
    }
  };
  return (
    <form
      class="row"
      onSubmit={(e) => {
        e.preventDefault();
        void start();
      }}
    >
      <input type="text" placeholder={`${who}'s room`} aria-label="The room's name" maxLength={80} value={name} onInput={(e) => setName((e.target as HTMLInputElement).value)} />
      <button class="btn primary" type="submit" disabled={busy}>
        Start a room
      </button>
      {err && <span class="error">{err}</span>}
    </form>
  );
}

export function Home({ me, onSignedOut }: { me: Session; onSignedOut: () => void }) {
  const account = me.account;
  const [guest, setGuest] = useState(kept(NAME));
  const [draft, setDraft] = useState(kept(NAME));
  const [list, setList] = useState<{ made: RoomInfo[]; recent: RoomInfo[] } | null>(null);
  useEffect(() => {
    if (account) void api.rooms().then(setList, () => setList({ made: [], recent: [] }));
  }, [account?.id]);
  const who = account ? kept(NAME) || accountName(account) : guest;
  const comeIn = () => {
    const n = draft.trim().slice(0, 80);
    if (!n) return;
    keep(NAME, n);
    setGuest(n);
  };
  const signOut = async () => {
    await api.signOut().catch(() => undefined);
    onSignedOut();
  };
  return (
    <div class="home">
      <header class="top">
        <Mark />
        {account ? (
          <span class="who">
            <span class="stats">{account.email ?? accountName(account)}</span>
            <button class="btn" onClick={() => void signOut()}>
              Sign out
            </button>
          </span>
        ) : guest ? (
          <span class="who">
            <span class="stats">here as {guest}</span>
            <button class="btn" onClick={() => setGuest("")}>
              Not you?
            </button>
          </span>
        ) : null}
      </header>
      <main>
        <section class="hero">
          <div>
            <h1 class="headline">
              Hang out <span>here.</span>
            </h1>
            <p class="lede">A room for your people. Everyone shows up as a little character that moves when they do. Wave, react, talk in the chat or on a call. No picture of you leaves your computer unless you start video in a call.</p>
          </div>
          <div class="stage cast" aria-hidden="true">
            {CHARACTERS.map((c, i) => (
              <Idle key={c.id} character={c.id} colour={(i * 2) % COLOURS.length} />
            ))}
          </div>
        </section>

        {who ? (
          <section class="card panel" style={{ "--c": "var(--k-server)" }}>
            <h2>Start a room</h2>
            <p class="sub">You'll get a link to send people. Anyone with it can come in.</p>
            <StartRoom who={who} />
          </section>
        ) : (
          <div class="ways">
            <section class="card panel" style={{ "--c": "var(--k-build)" }}>
              <h2>Come in as a guest</h2>
              <p class="sub">Just a name. You can start rooms and join any room you have a link to.</p>
              <form
                class="row"
                onSubmit={(e) => {
                  e.preventDefault();
                  comeIn();
                }}
              >
                <input type="text" placeholder="your name" aria-label="Your name" maxLength={80} value={draft} onInput={(e) => setDraft((e.target as HTMLInputElement).value)} />
                <button class="btn primary" type="submit" disabled={!draft.trim()}>
                  Continue as a guest
                </button>
              </form>
            </section>
            <section class="card panel" style={{ "--c": "var(--k-agent)" }}>
              <h2>Or sign in</h2>
              <p class="sub">Keep a list of the rooms you start and the ones you've been in.</p>
              <SignIn me={me} returnTo={path("/")} />
            </section>
          </div>
        )}

        {account && list && (
          <div class="lists">
            <RoomList title="Your rooms" rooms={list.made} kind="--k-server" empty="Rooms you start show up here." />
            <RoomList title="Lately" rooms={list.recent} kind="--k-agent" empty="Rooms you join show up here." />
          </div>
        )}
        {!account && guest && (
          <section class="card panel" style={{ "--c": "var(--k-agent)" }}>
            <h2>Keep your rooms</h2>
            <p class="sub">Sign in and the rooms you start and join stay in a list here.</p>
            <SignIn me={me} returnTo={path("/")} />
          </section>
        )}
      </main>
    </div>
  );
}
