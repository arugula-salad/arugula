// arugula.stream/developers: setting it up, in order. Try a demo room with
// no account, make an app for a key, put the room in a page, then decide
// who gets in (guests, your own sign-in, or your server's tokens), and
// calls come with it. The front page (/) is the hosted pitch; the docs
// are the reference. Nav and SOURCE live here for every page.

import { useState } from "preact/hooks";
import { Code, Tabs } from "./code";
import { cleanRoom, newRoom } from "./room";
import { FakeRoom } from "./hero";
import { browser, guests, install, providers, prototype, server } from "./snippets";

export const SOURCE = "https://github.com/arugula-salad/arugula/tree/presence-toolkit/presence";

export function Nav() {
  return (
    <nav class="nav">
      <a class="mark" href="/">
        arugula<span>.</span>stream
      </a>
      <a href="/developers">Developers</a>
      <a href="/docs">Docs</a>
      <a href={SOURCE}>Source</a>
      <a class="dash" href="/dashboard">
        Dashboard
      </a>
      <a class="btn" href={`/r/${newRoom()}`}>
        Try a room
      </a>
    </nav>
  );
}

function Step({ n, title, children }: { n: number; title: string; children: preact.ComponentChildren }) {
  return (
    <section class="dev step">
      <h2>
        <span class="step-n">{n}</span>
        {title}
      </h2>
      {children}
    </section>
  );
}

export function Developers() {
  const [room, setRoom] = useState("");
  const go = (r: string) => (location.href = `/r/${encodeURIComponent(cleanRoom(r) || newRoom())}`);
  return (
    <>
      <Nav />
      <main class="landing">
        <section class="hero">
          <div>
            <p class="eyebrow">For developers</p>
            <h1 class="headline">Presence in your app, in an afternoon.</h1>
            <p class="lede">
              Live avatars of who's around, drawn from each person's camera in their own browser, with text, waves, reactions and calls. Frontend code
              only: we run the live part.
            </p>
            <div class="start">
              <a class="btn primary" href="#try">
                Start setting up
              </a>
              <a class="btn" href="/docs">
                Read the docs
              </a>
            </div>
          </div>
          <div class="stage">
            <FakeRoom />
          </div>
        </section>

        <div id="try" />
        <Step n={1} title="Try it, no account">
          <p class="sub">Paste this into any page and open it in two browsers. Demo rooms are open to anyone who knows the name, so keep them for trying.</p>
          <Code code={prototype} label="A demo room" />
        </Step>

        <Step n={2} title="Make an app">
          <p class="sub">
            Sign in to the <a href="/dashboard">dashboard</a> and create an app. You get a publishable key (<code>pk_live_…</code>, safe to ship in
            your page) and a test key that also works from localhost. Add the sites your page runs on as allowed origins; a key only works from
            them.
          </p>
        </Step>

        <Step n={3} title="Decide who gets in">
          <p class="sub">Pick one. You can change your mind later without touching the rest.</p>
          <div class="ways">
            <div class="way card" style={{ "--c": "var(--k-test)" }}>
              <b>Guests</b>
              <span>Anyone with the link, by a name they type. Good for open rooms and trying it with real people.</span>
            </div>
            <div class="way card" style={{ "--c": "var(--k-agent)" }}>
              <b>Your sign-in</b>
              <span>Clerk, Auth0, Firebase, Supabase or any OIDC provider. We check your users' own tokens; you run no server.</span>
            </div>
            <div class="way card" style={{ "--c": "var(--k-server)" }}>
              <b>Your server</b>
              <span>Your server signs a short room token with your app's secret. Full control, a dozen lines in any language.</span>
            </div>
          </div>
          <h3>Guests</h3>
          <Tabs tabs={guests} label="Guests" install={install} />
          <h3>Your sign-in provider</h3>
          <p class="sub">Set your provider in the dashboard, then hand the room a fresh token from it:</p>
          <Tabs tabs={providers} label="Your sign-in" install={install} />
          <h3>Your server</h3>
          <p class="sub">The page asks your endpoint for a token; your endpoint decides who may be in which room:</p>
          <Tabs tabs={browser} label="In the browser" install={install} />
          <Tabs tabs={server} label="On your server" install={install} />
        </Step>

        <Step n={4} title="Wave, then talk">
          <p class="sub">
            Calls come with it. When someone waves and the other says Talk, they're in a voice call, in the same room, with presence carrying on
            underneath. Add <code>&lt;arugula-call&gt;</code> for the call bar (mute, leave), or use <code>room.call(sub)</code>. Calls are real audio
            and video; presence never sends a picture.
          </p>
        </Step>

        <section class="points">
          <div class="point">
            <b>Private by construction</b>
            <span>Face and pose tracking run in the browser. The relay passes on a rig or line outlines, never a picture, and keeps no frames.</span>
          </div>
          <div class="point">
            <b>Ambient, not a call</b>
            <span>Nobody's on camera. You can tell who's at their desk, who's laughing, who stepped away, and they can tell the same about you.</span>
          </div>
          <div class="point">
            <b>Small</b>
            <span>A rig is ~0.5 kbps per person, so a room of people costs less than one video tile.</span>
          </div>
          <div class="point">
            <b>Open</b>
            <span>
              The <a href={`${SOURCE}/PROTOCOL.md`}>protocol</a> is open, the client and relay are open source, and you can run it all yourself.
            </span>
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
        <a href="/">Home</a> · <a href="/docs">Docs</a> · <a href={`${SOURCE}/PROTOCOL.md`}>Protocol</a> · <a href={SOURCE}>Source</a> · <a href="/privacy">Privacy</a> · <a href="/terms">Terms</a> · from the makers of Arugula
      </footer>
    </>
  );
}
