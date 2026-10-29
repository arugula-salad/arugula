# arugula.stream SDKs: the surface (v0)

What a developer writes. The browser SDK and the server SDKs are built to
this; the site's docs show exactly this. PROTOCOL.md is underneath.

## Browser: one script tag (web components)

    <script type="module" src="https://arugula.stream/sdk/v0/presence.js"></script>

    <arugula-room room="design-team" token-url="/api/presence-token">
      <arugula-camera></arugula-camera>   <!-- camera on/off, avatar|outline, character, colour -->
      <arugula-people></arugula-people>   <!-- a tile per person in the room -->
    </arugula-room>

    <!-- anywhere inside a room: one person's live face, e.g. beside their name -->
    <arugula-avatar user="u_123" size="head"></arugula-avatar>

`<arugula-room>` attributes:
- `room` (required): the room's id in your app.
- `token-url`: your endpoint; the element POSTs `{room}` (JSON, with the page's cookies) and expects `{token}` (a room token your server signed). Called on every (re)connect.
- `demo` + `name`: prototype without a server: arugula.stream's public demo tokens (anyone who knows the room's name can join it). Not for production.
- `relay`: default `wss://arugula.stream/socket`.
- `mode` (`avatar`|`outline`, default `avatar`), `character`, `colour`: starting choices (the person can change them; kept in their browser).

`<arugula-people>`: attributes `size` (`bust`, default; `head` for a compact strip), `self` (`first`|`hide`). `<arugula-camera>`: attribute `controls` (`full`, default; `button` for just the toggle). `<arugula-avatar>`: `user`, `size` (`head`|`bust`). Theming: CSS custom properties on any ancestor: `--arugula-accent`, `--arugula-bg`, `--arugula-fg`, `--arugula-dim`, `--arugula-radius`.

Events (bubbling, on the room element): `arugula:members` (`detail.members`), `arugula:wave` (`detail.from`), `arugula:answer` (`detail.from, detail.yes`), `arugula:live` (`detail.state`).

## Browser: JS API (same bundle, or npm `@arugula/presence`)

    import { connect } from "https://arugula.stream/sdk/v0/presence.js";

    const room = await connect({
      room: "design-team",
      token: async () => (await fetch("/api/presence-token", { method: "POST", body: JSON.stringify({ room: "design-team" }) })).json().then((r) => r.token),
      // relay: "wss://arugula.stream/socket",
    });
    room.members;                       // [{ sub, name, look, mode, away, watching }]
    room.on("members", (members) => {}); // also "wave", "answer", "live"
    await room.camera.start({ mode: "avatar" });   // or "outline"
    room.camera.stop();
    room.setLook({ character: 2, colour: 0 });
    room.wave(sub); room.answer(sub, true);
    room.mount(element, { size: "bust" });          // draw everyone into an element
    room.avatar(element, sub, { size: "head" });    // draw one person
    room.leave();

## React: `@arugula/presence-react`

    <PresenceRoom room="design-team" token={getToken}>
      <CameraControls />
      <People />
      <Avatar user={id} size="head" />
    </PresenceRoom>
    const { members, wave, camera } = usePresence();

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
