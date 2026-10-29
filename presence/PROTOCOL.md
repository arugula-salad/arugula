# arugula.video presence protocol, v0

How a page shows who's around as live avatars or outlines, and how the
relay passes it on. The relay sees opaque frames and who's in a room;
no picture of anyone ever reaches it. Grew out of Arugula's S43 (outlines)
and S44 (rigged avatars) spikes; tracker Arugula #315.

## Pieces

- **Host app server**: signs a **room token** for each of its users with
  the app's secret. It alone decides who may be in which room.
- **Client** (`@arugula/presence`, `presence/client`): joins a room with a
  token, publishes the user's frames and state, receives everyone else's.
- **Relay** (`presence/relay`, Phoenix): rooms, membership, fan-out. It
  keeps each member's newest frame in memory for a joiner, and nothing
  else.

## Connecting

A Phoenix socket (serializer V2, JSON with binary pushes) at
`/socket/websocket?vsn=2.0.0&<params>`, where the params are one of:

| params | who decides who's in the room |
|---|---|
| `token=<room token>` | the host app's server, which signed it (below) |
| `key=<publishable key>&room=<room>&name=<name>[&look=<character>:<colour>]` | nobody: a **guest**, where the app allows guests |

The socket refuses a missing, expired or badly signed token, and a
publishable key that's unknown, revoked, or not from one of its app's
allowed origins. Either way the room channel is the same.

### Publishable keys

A hosted relay (arugula.stream) keeps apps in its database, each with
publishable keys (`pk_live_…`, `pk_test_…`), safe to put in a page:

- The socket's `Origin` header must match one of the app's allowed
  origins (`scheme://host[:port]`; `https://*.example.com` matches its
  subdomains, not the bare domain). A `pk_test_` key is also accepted
  from `localhost`, `127.0.0.1` and `[::1]` on any port. No `Origin`, no
  key: a key is for browsers.
- A guest gives only a `name` (rules as the token's `name`); the relay
  makes their `sub`, `g_` and 12 hex digits, new for each socket.
- `room` follows the token's `room` rules; the channel topic must match it.
- Keys are revoked one at a time; the app's server secret (for room
  tokens) is shown once and rotated on its own.

### Room token

A JWT, HS256, signed with the app's secret (the relay has each app's
secret by its `app` id, from its config or, hosted, its database; a demo
relay has one app, `demo`):

| claim | |
|---|---|
| `app` | the app's id |
| `room` | the room's id within the app (`[A-Za-z0-9_.:-]{1,128}`) |
| `sub` | the user's id within the app (`[A-Za-z0-9_.:@-]{1,128}`) |
| `name` | what to call them (≤ 80 chars) |
| `exp` | expiry, Unix seconds (a token lives ≤ 24 h) |
| `look` | optional `{character, colour}` (bytes) to start with |

A **demo relay** (`DEMO=1`) also hands out tokens for app `demo` from
`POST /api/demo/token` with `{room, name}`: anyone may join any demo room
by its name, and `sub` is random.

## The room channel

Topic `room:<room>` (the app comes from the token or the key; a socket
for another room is refused). Join payload: `{}`. The reply is `{you: <sub>}`.

### Presence

Phoenix.Presence on the topic: `presence_state` and `presence_diff` as
Phoenix sends them, keyed by `sub`, one meta per connection:

```json
{ "name": "Jake", "look": {"character": 2, "colour": 0},
  "mode": "avatar" | "outline" | "none", "away": false, "watching": true }
```

`mode` is what the member's camera sends (`none`: camera off). `watching`
is whether any of their pages is showing others (so a sender with nobody
watching can drop to a frame every few seconds).

### Client → relay

| event | payload | |
|---|---|---|
| `frame` | binary, ≤ 32 KiB | the member's newest frame (below). Frames closer than 70 ms to the member's last are dropped. |
| `state` | `{mode?, look?, away?, watching?}` | updates the member's presence meta |
| `wave` | `{to: <sub>}` | wave at someone: a hello that asks to talk (calls come later) |
| `answer` | `{to: <sub>, yes: bool}` | answer a wave |
| `say` | `{text}` | a text message for the room: 1–2000 characters after trimming, no control characters but newlines; at most 5 per 5 s per connection. Replies `ok` with the kept message, or `error` `{reason: "empty" \| "too long" \| "control characters" \| "too fast"}` (and it goes nowhere) |
| `history` | `{before: <id>}` | the 50 messages before that one; replies `ok` `{messages: [...]}`, oldest first |
| `react` | `{emoji}` | an emoji for the room (a 👍, a 🎉): one grapheme, no ASCII, ≤ 32 bytes, at most one per 250 ms per connection, else dropped |

### Relay → client

| event | payload | |
|---|---|---|
| `frame` | binary: `len:u8 sub:utf8[len] frame` | someone's frame, to everyone else in the room; on join, every member's newest frame younger than 10 s |
| `off` | `{sub}` | someone's camera went off, or their frame went stale (10 s) |
| `wave` | `{from: <sub>}` | to the waved member only |
| `answer` | `{from: <sub>, yes}` | to the waver only |
| `said` | `{id, sub, name, text, at}` | a message, to everyone in the room but the connection that said it (which has it in its reply); `id` only goes up in a room, `at` is Unix ms |
| `history` | `{messages: [...]}` | on join: the room's last 50 messages, oldest first |
| `react` | `{from: <sub>, emoji}` | someone's reaction, to everyone else in the room (not stored: a joiner doesn't get old ones) |

Limits: 32 KiB a frame, 70 ms between a member's frames, 64 members a room,
8 connections a member. Over a limit, the frame is dropped (no error);
too many members or connections, the join is refused (`{reason}`).

## Frames

Opaque to the relay. Byte 0 says how the rest is encoded (`0` raw, `1`
deflate-raw of raw); raw byte 0 says what it is:

- `1`–`3`: an **outline** (presence-core's `frame.ts`): polylines in grid
  cells, v2 with a class byte per line, v3 with a nearness byte.
- `0x40`: a **rig** (presence-core's `rig.ts`): head, face, shoulders,
  wrists, hands, place in frame, and the sender's look. 21–40 bytes.

A client that doesn't know a kind or version skips the frame.
