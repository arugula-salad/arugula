// arugula.stream/docs: everything a developer needs on one page, from a
// demo room in five lines to tokens in production (presence/sdk/API.md).

import { Code, Tabs } from "./code";
import { Nav, SOURCE } from "./landing";
import { browser, install, jsApi, prototype, reactHook, server, theming } from "./snippets";

const SECTIONS = [
  ["quickstart", "Quickstart"],
  ["tokens", "Room tokens"],
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
            App ids and secrets for the hosted relay are issued by hand while it's in preview. You can also run the relay yourself (
            <a href={`${SOURCE}/relay`}>presence/relay</a>, Phoenix) with your own <code>PRESENCE_APPS</code>.
          </p>

          <h2 id="elements">Elements</h2>
          <p>
            <code>{"<arugula-room>"}</code> joins a room; the other elements, inside it, show it.
          </p>
          <table class="attrs">
            <tbody>
              <Attr name="room">the room's id in your app (required)</Attr>
              <Attr name="token-url">your endpoint that returns {"{token}"}; called on every (re)connect</Attr>
              <Attr name="demo + name">a public demo room instead (prototypes only)</Attr>
              <Attr name="relay">default wss://arugula.stream/socket</Attr>
              <Attr name="mode, character, colour">starting choices; each person can change theirs</Attr>
            </tbody>
          </table>
          <table class="attrs">
            <tbody>
              <Attr name="<arugula-camera>">camera on/off, avatar or outline, character and colour; controls="button" for just the toggle</Attr>
              <Attr name="<arugula-people>">a tile per person; size="head" for a compact strip, self="hide" to leave yourself out</Attr>
              <Attr name="<arugula-avatar>">one person's live face: user="…" size="head|bust", e.g. beside their name in your UI</Attr>
            </tbody>
          </table>
          <p>
            Events bubble from the room element: <code>arugula:members</code>, <code>arugula:wave</code>, <code>arugula:answer</code>, <code>arugula:react</code>,{" "}
            <code>arugula:live</code>. Theme with custom properties on any ancestor:
          </p>
          <Code code={theming} label="Theming" />

          <h2 id="js">JS API</h2>
          <p>The same bundle exports the API the elements are built on, for when you draw your own UI.</p>
          <Code code={jsApi} label="JS API" />

          <h2 id="react">React</h2>
          <p>
            <code>@arugula/presence-react</code> wraps the API: <code>PresenceRoom</code>, <code>People</code>, <code>CameraControls</code>,{" "}
            <code>Avatar</code>, and a hook:
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
            <li>Reactions: any single emoji, for the room, at most four a second each; they float over the sender's tile and aren't kept.</li>
            <li>Calls (voice and video after a wave) are next; waving works today.</li>
          </ul>

          <h2 id="protocol">Protocol</h2>
          <p>
            Phoenix channels over a WebSocket: join <code>room:&lt;room&gt;</code> with a token, frames as binary pushes, presence, wave and answer, reactions.
            The whole thing is one page: <a href={`${SOURCE}/PROTOCOL.md`}>PROTOCOL.md</a>.
          </p>
        </article>
      </div>
    </>
  );
}
