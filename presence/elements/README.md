# @arugula/presence-elements

arugula.stream in a page: who's around as live avatars or outlines, with no
picture leaving anyone's browser. `connect()` for code, and four custom
elements. The same module, bundled, is served at
`https://arugula.stream/sdk/v0/presence.js` (built into the relay's
`priv/static/sdk/v0` by `pnpm build`). The surface: `../sdk/API.md`.

## Script tag

```html
<script type="module" src="https://arugula.stream/sdk/v0/presence.js"></script>

<arugula-room room="design-team" token-url="/api/presence-token">
  <arugula-camera></arugula-camera>   <!-- camera on/off, avatar|outline, character, colour -->
  <arugula-people></arugula-people>   <!-- a tile per person in the room -->
</arugula-room>

<!-- anywhere inside a room: one person's live face, e.g. beside their name -->
<arugula-avatar user="u_123" size="head"></arugula-avatar>
```

- `<arugula-room>`: `room` (required); `token-url` (the element POSTs `{room}` with the page's cookies and expects `{token}`, a room token your server signed: `../sdk`); or `demo` + `name` to prototype with arugula.stream's public demo tokens; `relay` (default `wss://arugula.stream/socket`); `mode` (`avatar`|`outline`), `character`, `colour` as starting choices.
- `<arugula-people>`: `size` (`bust`|`head`), `self` (`first`|`hide`). `<arugula-camera>`: `controls` (`full`|`button`). `<arugula-avatar>`: `user`, `size` (`head`|`bust`); size the element with CSS.
- Theme with `--arugula-accent`, `--arugula-bg`, `--arugula-fg`, `--arugula-dim`, `--arugula-radius` on any ancestor.
- Events on the room element (bubbling): `arugula:members` (`detail.members`), `arugula:wave` (`detail.from`), `arugula:answer` (`detail.from`, `detail.yes`), `arugula:live` (`detail.state`).

`example.html` is a page with nothing but the script tag, in demo mode.

## JS API

```js
import { connect } from "https://arugula.stream/sdk/v0/presence.js";

const room = await connect({
  room: "design-team",
  token: async () => (await fetch("/api/presence-token", { method: "POST", body: JSON.stringify({ room: "design-team" }) })).json().then((r) => r.token),
  // relay: "wss://arugula.stream/socket",
});
room.members;                        // [{ sub, name, look, mode, away, watching }]
room.on("members", (members) => {}); // also "wave", "answer", "live"
await room.camera.start({ mode: "avatar" });   // or "outline"
room.camera.stop();
room.setLook({ character: 2, colour: 0 });
room.wave(sub); room.answer(sub, true);
room.mount(element, { size: "bust" });          // draw everyone into an element
room.avatar(element, sub, { size: "head" });    // draw one person
room.leave();
```

`mount` and `avatar` draw into a shadow root of their own (the elements'
styles); each returns a function that stops it. The trackers' models load
from the relay's origin unless `connect({ assets })` says otherwise.

## Developing

`pnpm test` (node, a fake transport), `pnpm typecheck`, `pnpm build` (the
demo builds `priv/static` first; this adds `sdk/v0`). The relay answers
`/sdk/**`, `/mediapipe/**`, `/depth/**` and `POST /api/demo/token` to any
origin (CORS), so the script tag works from any site.
