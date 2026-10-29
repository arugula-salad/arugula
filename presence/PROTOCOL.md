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
`/socket/websocket?vsn=2.0.0&token=<room token>`. The socket refuses a
missing, expired or badly signed token.

### Room token

A JWT, HS256, signed with the app's secret (the relay has each app's
secret by its `app` id; a demo relay has one app, `demo`):

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

Topic `room:<room>` (the app comes from the token; a token for another
room is refused). Join payload: `{}`. The reply is `{you: <sub>}`.

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
| `knock` | `{to: <sub>}` | ask someone to talk (calls come later) |
| `answer` | `{to: <sub>, yes: bool}` | answer a knock |

### Relay → client

| event | payload | |
|---|---|---|
| `frame` | binary: `len:u8 sub:utf8[len] frame` | someone's frame, to everyone else in the room; on join, every member's newest frame younger than 10 s |
| `off` | `{sub}` | someone's camera went off, or their frame went stale (10 s) |
| `knock` | `{from: <sub>}` | to the knocked member only |
| `answer` | `{from: <sub>, yes}` | to the knocker only |

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
