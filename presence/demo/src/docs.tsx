// arugula.stream/docs: everything a developer needs on one page, from a
// demo room in five lines to tokens in production (presence/sdk/API.md).

import { Code, Tabs } from "./code";
import { Nav, SOURCE } from "./landing";
import { browser, install, jsApi, prototype, providers, reactHook, roomRule, server, theming } from "./snippets";

const SECTIONS = [
  ["quickstart", "Quickstart"],
  ["tokens", "Room tokens"],
  ["sign-in", "Your sign-in, no server"],
  ["elements", "Elements"],
  ["js", "JS API"],
  ["react", "React"],
  ["server", "Server SDKs"],
  ["limits", "Limits and privacy"],
  ["protocol", "Protocol"],
] as const;

function Attr({ name, children }: { name: string; children: preact.ComponentChildren }) {
  return (
    <tr>
      <td>
        <code>{name}</code>
      </td>
      <td>{children}</td>
    </tr>
  );
}

export function Docs() {
  return (
    <>
      <Nav />
      <div class="docs">
        <aside class="toc">
          {SECTIONS.map(([id, title]) => (
            <a href={`#${id}`}>{title}</a>
          ))}
        </aside>
        <article>
          <h1 id="quickstart">Quickstart</h1>
          <p>
            Two steps: drop the elements into your page, and give them a token from your server. To try it before writing any server code, use a
            demo room:
          </p>
          <Code code={prototype} label="A demo room" />
          <p>
            Open the page in two browsers. Each person turns on their camera and picks a character; everyone in the room sees everyone else, live.
            For your app, swap <code>demo</code> for <code>token-url</code>:
          </p>
          <Tabs tabs={browser} label="In the browser" install={install} />

          <h2 id="tokens">Room tokens</h2>
          <p>
            Your server decides who may be in which room. When the page joins, it asks your <code>token-url</code> (a POST with{" "}
            <code>{"{room}"}</code>, sent with the page's cookies) and you answer <code>{"{token}"}</code>: a JWT, HS256, signed with your app's secret,
            naming the app, the room, the user's id and name, and an expiry (at most 24 h). The relay verifies the signature and nothing else.
          </p>
          <Tabs tabs={server} label="Sign a token" install={install} />
          <p class="note">
            Make an app in <a href="/dashboard">the dashboard</a> for its id, secret and publishable keys. You can also run the relay yourself (
            <a href={`${SOURCE}/relay`}>presence/relay</a>, Phoenix) with your own <code>PRESENCE_APPS</code>.
          </p>

          <h2 id="sign-in">Your sign-in, no server</h2>
          <p>
            Already signing people in with Clerk, Auth0, Firebase, Supabase or another OpenID Connect provider? Then you need no server for this. Set
            your app's issuer, and the page joins with its publishable key and the person's own token, which the relay checks against the
            provider's published keys. <code>auth</code> is a token or a function for a fresh one, called on every (re)connect.
          </p>
          <Tabs tabs={providers} label="Join with your sign-in" />
          <p>
            Each member's id is their provider id, scoped to the issuer; their name comes from the token's <code>name</code> claim (Supabase:{" "}
            <code>user_metadata.full_name</code>), else the <code>name</code> you pass. The key still only works from your app's origins, and guests
            (by name, if you allow them) still can. A room rule keeps members to their rooms by a claim:
          </p>
          <Code code={roomRule} label="A room rule" />

          <h2 id="elements">Elements</h2>
          <p>
            <code>{"<arugula-room>"}</code> joins a room; the other elements, inside it, show it.
          </p>
          <table class="attrs">
            <tbody>
              <Attr name="room">the room's id in your app (required)</Attr>
              <Attr name="token-url">your endpoint that returns {"{token}"}; called on every (re)connect</Attr>
              <Attr name="key + auth">no server: your publishable key and the person's sign-in token (or a bare auth, then el.auth = () =&gt; getToken())</Attr>
              <Attr name="key + name">a guest by name, where your app allows guests</Attr>
              <Attr name="demo + name">a public demo room instead (prototypes only)</Attr>
              <Attr name="relay">default wss://arugula.stream/socket</Attr>
              <Attr name="mode, character, colour">starting choices; each person can change theirs</Attr>
            </tbody>
          </table>
          <table class="attrs">
            <tbody>
              <Attr name="<arugula-camera>">camera on/off, avatar or outline, character and colour; controls="button" for just the toggle</Attr>
              <Attr name="<arugula-people>">a tile per person; size="head" for a compact strip, self="hide" to leave yourself out</Attr>
              <Attr name="<arugula-chat>">the room's text: messages (kept, with history) and an input; Enter sends, Shift+Enter is a new line</Attr>
              <Attr name="<arugula-avatar>">one person's live face: user="…" size="head|bust", e.g. beside their name in your UI</Attr>
              <Attr name="<arugula-call>">the call bar: a ring to answer, who you're calling, mute and leave; empty without a call. People's tiles get a Call button where the relay has calls (calls="off" on {"<arugula-people>"} hides it)</Attr>
            </tbody>
          </table>
          <p>
            Events bubble from the room element: <code>arugula:members</code>, <code>arugula:wave</code>, <code>arugula:answer</code>, <code>arugula:react</code>, <code>arugula:message</code>,{" "}
            <code>arugula:ring</code>, <code>arugula:call</code>, <code>arugula:live</code>. Theme with custom properties on any ancestor:
          </p>
          <Code code={theming} label="Theming" />

          <h2 id="js">JS API</h2>
          <p>The same bundle exports the API the elements are built on, for when you draw your own UI.</p>
          <Code code={jsApi} label="JS API" />

          <h2 id="react">React</h2>
          <p>
            <code>@arugula/presence-react</code> wraps the API: <code>PresenceRoom</code>, <code>People</code>, <code>CameraControls</code>,{" "}
            <code>Avatar</code>, <code>Chat</code>, <code>CallControls</code>, and a hook:
          </p>
          <Code code={browser.React} label="React" />
          <Code code={reactHook} label="The hook" />

          <h2 id="server">Server SDKs</h2>
          <p>
            One function in each, the same inputs, byte-identical tokens (they're tested against shared vectors and the relay). Zero dependencies
            beyond the standard library.
          </p>
          <table class="attrs">
            <tbody>
              <Attr name="Node / TypeScript">createRoomToken({"{ app, secret, room, user: { id, name }, look?, ttl? }"})</Attr>
              <Attr name="Python">create_room_token(app=, secret=, room=, user_id=, name=, look=None, ttl=3600)</Attr>
              <Attr name="Go">presence.CreateRoomToken(presence.TokenOptions{"{…}"})</Attr>
              <Attr name="Elixir">ArugulaPresence.create_room_token(app: , secret: , room: , user_id: , name: )</Attr>
              <Attr name="Ruby">ArugulaPresence.create_room_token(app:, secret:, room:, user_id:, name:)</Attr>
            </tbody>
          </table>
          <p class="note">
            Each installs from arugula.stream in one line (Elixir from the repo), shown above each example. The source is in{" "}
            <a href={`${SOURCE}/sdk`}>presence/sdk</a>; each SDK is one file under a hundred lines, so copying it in works too.
          </p>

          <h2 id="limits">Limits and privacy</h2>
          <ul>
            <li>Tracking runs in the browser (MediaPipe; WebGPU for depth). The camera's picture never leaves the page.</li>
            <li>What's sent: a rig (21–40 bytes, ~12 a second) or line outlines (a few KB, ~4 a second). The relay keeps each person's newest frame in memory for someone joining, and nothing else.</li>
            <li>64 people a room, 32 KiB a frame, a frame per 70 ms per person.</li>
            <li>Text: 1–2000 characters a message, at most 5 per 5 seconds each; kept, the last 50 on joining and earlier pages on request.</li>
            <li>Reactions: any single emoji, for the room, at most four a second each; they float over the sender's tile and aren't kept.</li>
            <li>Calls: a wave they say yes to is a voice call (LiveKit; audio only for now, video next). The relay rings, at most 3 rings per 30 seconds each, and hands each person a 10-minute token for that call's room alone; audio never passes through it.</li>
          </ul>

          <h2 id="protocol">Protocol</h2>
          <p>
            Phoenix channels over a WebSocket: join <code>room:&lt;room&gt;</code> with a token, frames as binary pushes, presence, wave and answer, calls, reactions.
            The whole thing is one page: <a href={`${SOURCE}/PROTOCOL.md`}>PROTOCOL.md</a>.
          </p>
        </article>
      </div>
    </>
  );
}
