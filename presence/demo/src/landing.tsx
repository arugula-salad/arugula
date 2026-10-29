// arugula.stream's front page, developers first: what it is in a line,
// the three lines that put it in an app, the token their server signs,
// then how it works and a room to try.

import { useState } from "preact/hooks";
import { Code, Tabs } from "./code";
import { cleanRoom, newRoom } from "./room";
import { FakeRoom } from "./hero";
import { browser, install, prototype, server } from "./snippets";

export const SOURCE = "https://github.com/arugula-salad/arugula/tree/presence-toolkit/presence";

export function Nav() {
  return (
    <nav class="nav">
      <a class="mark" href="/">
        arugula<span>.</span>stream
      </a>
      <a href="/hosted">Hosted</a>
      <a href="/docs">Docs</a>
      <a href={`${SOURCE}/PROTOCOL.md`}>Protocol</a>
      <a href={SOURCE}>Source</a>
      <a class="btn" href={`/r/${newRoom()}`}>
        Try a room
      </a>
    </nav>
  );
}

export function Landing() {
  const [room, setRoom] = useState("");
  const go = (r: string) => (location.href = `/r/${encodeURIComponent(cleanRoom(r) || newRoom())}`);
  return (
    <>
      <Nav />
      <main class="landing">
        <section class="hero">
          <div>
            <h1 class="headline">Presence for your app. Upgrades to a call.</h1>
            <p class="lede">
              Live avatars of who's around, drawn from each person's camera in their own browser. About twenty bytes a frame leave it; no picture
              ever does. When two people want to talk, they're already there.
            </p>
            <div class="start">
              <a class="btn primary" href="/docs#quickstart">
                Read the quickstart
              </a>
              <a class="btn" href={`/r/${newRoom()}`}>
                Try a room
              </a>
            </div>
          </div>
          <FakeRoom />
        </section>

        <section class="dev">
          <h2>Add it in three lines</h2>
          <p class="sub">One script tag, no build step. Or the JS API, or React components. Each person picks a character; their camera drives it.</p>
          <Tabs tabs={browser} label="In the browser" install={install} />
        </section>

        <section class="dev">
          <h2>Your server signs who's in which room</h2>
          <p class="sub">
            A room token is a short JWT your server signs with your app's secret. Your auth decides who joins; the relay only checks the signature.
            A dozen lines in any language.
          </p>
          <Tabs tabs={server} label="On your server" install={install} />
        </section>

        <section class="dev">
          <h2>No server yet? Prototype with demo rooms</h2>
          <p class="sub">Anyone who knows a demo room's name can join it, so keep them for trying things out.</p>
          <Code code={prototype} label="A demo room" />
        </section>

        <section class="points">
          <div class="point">
            <b>Private by construction</b>
            <span>Face and pose tracking run in the browser. The relay passes on a rig or line outlines, never a picture, and keeps nothing.</span>
          </div>
          <div class="point">
            <b>Ambient, not a call</b>
            <span>Nobody's on camera. You can tell who's at their desk, who's laughing, who stepped away, and they can tell the same about you.</span>
          </div>
          <div class="point">
            <b>Wave to talk</b>
            <span>Presence is the way in: wave at someone, they wave back, and you're talking. Voice and video calls build on that next.</span>
          </div>
          <div class="point">
            <b>Small and open</b>
            <span>A rig is ~0.5 kbps per person. The protocol is open, the client is open source, and you can run the relay yourself.</span>
          </div>
        </section>

        <section class="try">
          <h2>See it with someone</h2>
          <div class="start">
            <button class="btn primary" onClick={() => go(newRoom())}>
              Start a room
            </button>
            <input type="text" placeholder="or a room's name" value={room} onInput={(e) => setRoom((e.target as HTMLInputElement).value)} onKeyDown={(e) => e.key === "Enter" && room && go(room)} />
            {room && (
              <button class="btn" onClick={() => go(room)}>
                Join
              </button>
            )}
          </div>
        </section>
      </main>
      <footer class="foot">
        <a href="/docs">Docs</a> · <a href={`${SOURCE}/PROTOCOL.md`}>Protocol</a> · <a href={SOURCE}>Source</a> · from the makers of Arugula
      </footer>
    </>
  );
}
