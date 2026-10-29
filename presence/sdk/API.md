# arugula.stream SDKs: the surface (v0)

What a developer writes. The browser SDK and the server SDKs are built to
this; the site's docs show exactly this. PROTOCOL.md is underneath.

## Browser: one script tag (web components)

    <script type="module" src="https://arugula.stream/sdk/v0/presence.js"></script>

    <arugula-room room="design-team" token-url="/api/presence-token">
      <arugula-camera></arugula-camera>   <!-- camera on/off, avatar|outline, character, colour -->
      <arugula-people></arugula-people>   <!-- a tile per person in the room -->
      <arugula-chat></arugula-chat>       <!-- the room's text: messages and an input -->
      <arugula-call></arugula-call>       <!-- the call bar: a ring to answer, mute, video, leave -->
    </arugula-room>

    <!-- anywhere inside a room: one person's live face, e.g. beside their name -->
    <arugula-avatar user="u_123" size="head"></arugula-avatar>

`<arugula-room>` attributes:
- `room` (required): the room's id in your app.
- `token-url`: your endpoint; the element POSTs `{room}` (JSON, with the page's cookies) and expects `{token}` (a room token your server signed). Called on every (re)connect.
- `key` + `auth`: no server of yours: the app's publishable key, and the person's token from your auth provider (below). `auth="eyJ…"` is a token; a bare `auth` waits for the `auth` property, `el.auth = () => getToken()`, called on every (re)connect.
- `key` + `name`: a guest by name, where the app allows guests (with `auth`, `name` is only for a token without one).
- `demo` + `name`: prototype without a server: arugula.stream's public demo tokens (anyone who knows the room's name can join it). Not for production.
- `relay`: default `wss://arugula.stream/socket`.
- `mode` (`avatar`|`outline`, default `avatar`), `character`, `colour`: starting choices (the person can change them; kept in their browser).

`<arugula-people>`: attributes `size` (`bust`, default; `head` for a compact strip), `self` (`first`|`hide`), `calls` (`off` hides the Call button on tiles; it shows only where the relay has calls). `<arugula-camera>`: attribute `controls` (`full`, default; `button` for just the toggle). `<arugula-avatar>`: `user`, `size` (`head`|`bust`). Theming: CSS custom properties on any ancestor: `--arugula-accent`, `--arugula-bg`, `--arugula-fg`, `--arugula-dim`, `--arugula-radius`.

Events (bubbling, on the room element): `arugula:members` (`detail.members`), `arugula:wave` (`detail.from`), `arugula:answer` (`detail.from, detail.yes`), `arugula:react` (`detail.from, detail.emoji`), `arugula:message` (`detail.message`), `arugula:ring` (`detail.from, detail.id`), `arugula:call` (`detail.call`, null when it's over), `arugula:live` (`detail.state`). `<arugula-camera>` also has a row of reactions.

## Browser: JS API (same bundle, or npm `@arugula/presence`)

    import { connect } from "https://arugula.stream/sdk/v0/presence.js";

    const room = await connect({
      room: "design-team",
      token: async () => (await fetch("/api/presence-token", { method: "POST", body: JSON.stringify({ room: "design-team" }) })).json().then((r) => r.token),
      // or, no server: key: "pk_live_…", auth: () => getToken()   (or guest: "Ada")
      // relay: "wss://arugula.stream/socket",
    });
    room.members;                       // [{ sub, name, look, mode, away, watching, call }]
    room.on("members", (members) => {}); // also "wave", "answer", "react", "message", "ring", "call", "live"
    await room.camera.start({ mode: "avatar" });   // or "outline"
    room.camera.stop();
    room.setLook({ character: 2, colour: 0 });
    room.wave(sub); room.answer(sub, true);
    room.react("👍");                              // an emoji for the room
    await room.say("hi");                          // a kept text message
    room.messages;                                 // [{ id, sub, name, text, at }], oldest first
    await room.loadEarlier();                      // the 50 before the first one here
    room.chat(element);                            // draw the chat into an element
    room.calls;                                    // whether the relay has calls (LiveKit keys)
    await room.call(sub);                          // ask to talk: a voice call once they say yes
    room.on("ring", (from, id) => room.answerCall(id, true));
    room.inCall;                                   // { id, state, ringing, people: [{ sub, name, muted, speaking, video }], muted, video }
    room.setMuted(true); room.leaveCall();
    await room.setVideo(true);                     // your camera's real video, to the people in the call
    room.callBar(element);                         // draw the call bar into an element
    room.mount(element, { size: "bust" });          // draw everyone into an element
    room.avatar(element, sub, { size: "head" });    // draw one person
    room.leave();

## React: `@arugula/presence-react`

    <PresenceRoom room="design-team" token={getToken}>   {/* or publishableKey="pk_live_…" auth={getToken} */}
      <CameraControls />
      <People />
      <Avatar user={id} size="head" />
      <CallControls />
    </PresenceRoom>
    const { members, messages, say, wave, camera, calls, rings, inCall, call, answerCall, leaveCall, setMuted, setVideo } = usePresence();

**Video in a call is real video.** Presence sends a rig or an outline,
never a picture. A call's video (`setVideo(true)`, or the call bar's Start
video) is the camera's actual picture, sent to the people in that call
while it's on; tiles in the call show it in place of the character. It
shares the presence camera (no second camera prompt), with simulcast and
adaptive stream so each tile gets only the size it shows.

## Publishable keys (hosted, no server of yours)

On arugula.stream an app also has publishable keys (`pk_live_…` for its
allowed origins, `pk_test_…` for those and localhost), safe in a page.
The key only works from the origins you list for the app. Keys are
revocable.

**Signed-in members, with your own auth provider.** Set the app's issuer
(Clerk, Auth0, Firebase, Supabase or any OIDC provider; PROTOCOL.md,
"Members by sign-in"), then hand `connect()` the person's token from it.
`auth` is a token or a function for a fresh one, called on every
(re)connect; the relay checks it against the provider's keys, and the
member's id is their provider id, scoped to the issuer:

    connect({ room: "design-team", key: "pk_live_…", auth: () => getToken() })

    <arugula-room room="design-team" key="pk_live_…" auth></arugula-room>
    <script>document.querySelector("arugula-room").auth = () => getToken()</script>

    <PresenceRoom room="design-team" publishableKey="pk_live_…" auth={getToken}>

Where the token comes from, for each provider (and the issuer to set):

| provider | `auth` | issuer settings |
|---|---|---|
| Clerk | `() => Clerk.session.getToken()` | `{provider: "clerk", issuer: "https://<your Frontend API>"}`; add `"name": "{{user.full_name}}"` under Customize session token (or pass `name`) |
| Auth0 | `() => auth0.getTokenSilently()` (with `authorizationParams.audience`) | `{provider: "auth0", issuer: "<tenant>.auth0.com", audience: "<API identifier>"}` |
| Firebase | `() => getAuth().currentUser.getIdToken()` | `{provider: "firebase", project: "<project id>"}` |
| Supabase | `async () => (await supabase.auth.getSession()).data.session.access_token` | `{provider: "supabase", issuer: "https://<ref>.supabase.co"}` (asymmetric signing keys; legacy HS256 projects aren't supported) |
| OIDC | `async () => (await userManager.getUser()).id_token` | `{provider: "oidc", issuer: "<iss>", audience: "<client id>"}` |

A room rule keeps members to their rooms by a claim: `{"claim":
"org_id", "match": "prefix"}` admits org `acme`'s members to
`acme:standup` and no other org's rooms; `"match": "contains"` wants the
room in a list claim.

**Guests**, where the app allows them, come by name; the relay makes
their ids:

    connect({ room: "design-team", key: "pk_live_…", guest: "Ada" })

Where your server decides who's in which room, sign room tokens instead:

## Server: sign a room token (every language, same shape)

    createRoomToken({ app, secret, room, user: { id, name }, look?, ttl = 3600 }) -> string

An HS256 JWT. Claims are serialized compact (no spaces), in this order:
`app, room, sub, name, exp` then `look` (`{"character":n,"colour":n}`) if
given. The header is exactly `{"alg":"HS256","typ":"JWT"}`. base64url
without padding. So every SDK makes byte-identical tokens for the same
inputs (sdk/vectors.json), and the relay verifies them.

- Node/TS: `presence/sdk/node` (`@arugula/presence-server`): `createRoomToken({...})`
- Python: `presence/sdk/python` (`arugula_presence`): `create_room_token(app=, secret=, room=, user_id=, name=, look=None, ttl=3600)`
- Go: `presence/sdk/go` (`arugula.stream/presence`): `presence.CreateRoomToken(presence.TokenOptions{...})`
- Elixir: `presence/sdk/elixir` (`:arugula_presence`): `ArugulaPresence.create_room_token(app: , secret: , room: , user_id: , name: , look: nil, ttl: 3600)`
- Ruby: `presence/sdk/ruby` (`arugula_presence`): `ArugulaPresence.create_room_token(app:, secret:, room:, user_id:, name:, look: nil, ttl: 3600)`

Each has a `now`/clock parameter for tests and zero dependencies beyond
the language's standard library (Elixir: :crypto + Jason, which Phoenix
apps already have). Each validates as PROTOCOL.md does, a little stricter
so every SDK's token is byte-identical:
- app and room ids: `[A-Za-z0-9_.:-]{1,128}`; user ids `[A-Za-z0-9_.:@-]{1,128}`;
- a name: 1–80 code points, no control characters (U+0000–001F, U+007F)
  and no U+2028/2029 (JSON encoders escape those differently);
- ttl 1–86400 seconds (Go: `TTL time.Duration`, zero is an hour);
- look: character and colour 0–255.
Node ships plain ESM `index.js` with a handwritten `index.d.ts` (no build).
